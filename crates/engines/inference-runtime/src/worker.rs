//! Bounded single-thread host wrapper around the synchronous runtime registry.

use std::collections::BTreeMap;
use std::time::Duration;

use domain_contracts::{ModelHandle, ModelId, ModelLifecycleState, ModelLoader, MonotonicMillis};
use host_runtime::{
    BoundedReceiver, BoundedSender, HostThread, MonotonicClock, ReceiveTimeoutError,
    SendTimeoutError, ThreadPanicked, ThreadSpawnError, TryReceiveError, TrySendError, bounded,
    spawn_named,
};

use crate::{
    CommandTicket, HostedRuntimeConfiguration, InferenceRuntime, RuntimeCommand, RuntimeEvent,
    RuntimeLimits, RuntimeReceiveError, RuntimeSubmitError,
};

/// Client-side bounded command and event endpoints.
pub struct HostedRuntime<S> {
    commands: BoundedSender<RuntimeCommand<S>>,
    events: BoundedReceiver<RuntimeEvent>,
}

impl<S> HostedRuntime<S> {
    /// Attempts to submit a command without blocking.
    ///
    /// # Errors
    ///
    /// Returns the command if the bounded queue is full or the worker has disconnected.
    pub fn try_submit(&self, command: RuntimeCommand<S>) -> Result<(), RuntimeSubmitError<S>> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(command) => RuntimeSubmitError::Full(command),
                TrySendError::Disconnected(command) => RuntimeSubmitError::Disconnected(command),
            })
    }

    /// Attempts to receive one event without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeReceiveError::Timeout`] if the queue is empty, or
    /// [`RuntimeReceiveError::Disconnected`] if the worker has stopped.
    pub fn try_receive(&self) -> Result<RuntimeEvent, RuntimeReceiveError> {
        self.events.try_receive().map_err(|error| match error {
            TryReceiveError::Empty => RuntimeReceiveError::Timeout,
            TryReceiveError::Disconnected => RuntimeReceiveError::Disconnected,
        })
    }

    /// Waits up to `timeout` for one runtime event.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeReceiveError::Timeout`] if no event arrives before `timeout`,
    /// or [`RuntimeReceiveError::Disconnected`] if the worker has stopped.
    pub fn receive_timeout(&self, timeout: Duration) -> Result<RuntimeEvent, RuntimeReceiveError> {
        self.events
            .receive_timeout(timeout)
            .map_err(|error| match error {
                ReceiveTimeoutError::Timeout => RuntimeReceiveError::Timeout,
                ReceiveTimeoutError::Disconnected => RuntimeReceiveError::Disconnected,
            })
    }

    /// Returns the number of currently queued commands.
    #[must_use]
    pub fn queued_commands(&self) -> usize {
        self.commands.len()
    }

    /// Returns the number of currently queued events.
    #[must_use]
    pub fn queued_events(&self) -> usize {
        self.events.len()
    }
}

/// Correlation state retained while one model unload is asynchronous.
#[derive(Clone, Copy)]
struct PendingUnload {
    handle: ModelHandle,
    ticket: CommandTicket,
    failure_reported: bool,
}

/// Join handle for the exclusively owning runtime worker.
pub struct RuntimeThread {
    thread: HostThread<()>,
}

impl RuntimeThread {
    /// Reports whether the runtime worker has completed without blocking.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Waits for worker termination.
    ///
    /// # Errors
    ///
    /// Returns [`ThreadPanicked`] if the runtime worker panicked.
    pub fn join(self) -> Result<(), ThreadPanicked> {
        self.thread.join()
    }
}

/// Starts one bounded runtime worker around a concrete loader and model type.
///
/// # Errors
///
/// Returns [`ThreadSpawnError`] if the host cannot spawn the runtime worker thread.
pub fn start_hosted_runtime<L>(
    loader: L,
    limits: RuntimeLimits,
    configuration: HostedRuntimeConfiguration,
) -> Result<(HostedRuntime<L::Source>, RuntimeThread), ThreadSpawnError>
where
    L: ModelLoader + Send + 'static,
    L::Source: Send + 'static,
{
    let (command_sender, command_receiver) = bounded(configuration.command_capacity);
    let (event_sender, event_receiver) = bounded(configuration.event_capacity);
    let thread = spawn_named("llm-inference-runtime", move || {
        run_worker(
            InferenceRuntime::new(loader, limits),
            &command_receiver,
            &event_sender,
            configuration.poll_interval(),
        );
    })?;

    Ok((
        HostedRuntime {
            commands: command_sender,
            events: event_receiver,
        },
        RuntimeThread { thread },
    ))
}

fn run_worker<L>(
    mut runtime: InferenceRuntime<L>,
    commands: &BoundedReceiver<RuntimeCommand<L::Source>>,
    events: &BoundedSender<RuntimeEvent>,
    poll_interval: Duration,
) where
    L: ModelLoader,
{
    let clock = MonotonicClock::new();
    let mut pending_event = None;
    let mut stop_after_event = false;
    let mut pending_unloads = BTreeMap::<ModelId, PendingUnload>::new();
    let mut maintenance_events = BTreeMap::<ModelId, RuntimeEvent>::new();

    loop {
        if let Some((model_id, event)) =
            maintenance_event(&mut runtime, clock.now(), &mut pending_unloads)
        {
            maintenance_events.insert(model_id, event);
        }
        collect_naturally_completed_unloads(
            &runtime,
            &mut pending_unloads,
            &mut maintenance_events,
        );
        if pending_event.is_none() {
            pending_event = maintenance_events.pop_first().map(|(_, event)| event);
        }

        if let Some(event) = pending_event.take() {
            match events.send_timeout(event, poll_interval) {
                Ok(()) => {
                    if stop_after_event {
                        break;
                    }
                    continue;
                }
                Err(SendTimeoutError::Timeout(event)) => {
                    pending_event = Some(event);
                    continue;
                }
                Err(SendTimeoutError::Disconnected(_)) => {
                    let _shutdown_result = runtime.shutdown();
                    break;
                }
            }
        }

        match commands.receive_timeout(poll_interval) {
            Ok(command) => {
                let unload_identity = unload_command_identity(&command);
                let (event, should_stop) = dispatch(&mut runtime, command, clock.now());
                remember_pending_unload(unload_identity, &event, &runtime, &mut pending_unloads);
                pending_event = Some(event);
                stop_after_event = should_stop;
            }
            Err(ReceiveTimeoutError::Timeout) => {}
            Err(ReceiveTimeoutError::Disconnected) => {
                let _shutdown_result = runtime.shutdown();
                break;
            }
        }
    }
}

fn maintenance_event<L>(
    runtime: &mut InferenceRuntime<L>,
    now: MonotonicMillis,
    pending_unloads: &mut BTreeMap<ModelId, PendingUnload>,
) -> Option<(ModelId, RuntimeEvent)>
where
    L: ModelLoader,
{
    let (handle, result) = runtime.poll_unload_transition(now)?;
    let pending = pending_unloads.get(&handle.id).copied()?;
    match result {
        Ok(receipt) => {
            pending_unloads.remove(&handle.id);
            Some((
                handle.id,
                RuntimeEvent::ModelUnload {
                    ticket: pending.ticket,
                    result: Ok(receipt),
                },
            ))
        }
        Err(error) if !pending.failure_reported => {
            if let Some(pending) = pending_unloads.get_mut(&handle.id) {
                pending.failure_reported = true;
            }
            Some((
                handle.id,
                RuntimeEvent::ModelUnload {
                    ticket: pending.ticket,
                    result: Err(error),
                },
            ))
        }
        Err(_) => None,
    }
}

fn collect_naturally_completed_unloads<L>(
    runtime: &InferenceRuntime<L>,
    pending_unloads: &mut BTreeMap<ModelId, PendingUnload>,
    events: &mut BTreeMap<ModelId, RuntimeEvent>,
) where
    L: ModelLoader,
{
    let completed = pending_unloads
        .iter()
        .filter_map(|(model_id, pending)| {
            runtime
                .model_lifecycle_state(*model_id)
                .is_none()
                .then_some((*model_id, *pending))
        })
        .collect::<Vec<_>>();

    for (model_id, pending) in completed {
        pending_unloads.remove(&model_id);
        events.insert(
            model_id,
            RuntimeEvent::ModelUnload {
                ticket: pending.ticket,
                result: Ok(crate::UnloadReceipt {
                    handle: pending.handle,
                    status: crate::UnloadStatus::Unloaded,
                    cancelled_requests: 0,
                }),
            },
        );
    }
}

const fn unload_command_identity<S>(
    command: &RuntimeCommand<S>,
) -> Option<(ModelHandle, CommandTicket)> {
    match command {
        RuntimeCommand::UnloadModel { ticket, handle, .. } => Some((*handle, *ticket)),
        _ => None,
    }
}

fn remember_pending_unload<L>(
    identity: Option<(ModelHandle, CommandTicket)>,
    event: &RuntimeEvent,
    runtime: &InferenceRuntime<L>,
    pending_unloads: &mut BTreeMap<ModelId, PendingUnload>,
) where
    L: ModelLoader,
{
    let Some((handle, ticket)) = identity else {
        return;
    };
    let model_id = handle.id;
    let pending = runtime
        .model_lifecycle_state(model_id)
        .is_some_and(|state| {
            matches!(
                state,
                ModelLifecycleState::Draining { .. }
                    | ModelLifecycleState::Cancelling { .. }
                    | ModelLifecycleState::Unloading
            )
        });
    if pending {
        let failure_reported = matches!(event, RuntimeEvent::ModelUnload { result: Err(_), .. });
        pending_unloads.insert(
            model_id,
            PendingUnload {
                handle,
                ticket,
                failure_reported,
            },
        );
    } else {
        pending_unloads.remove(&model_id);
    }
}

fn dispatch<L>(
    runtime: &mut InferenceRuntime<L>,
    command: RuntimeCommand<L::Source>,
    now: MonotonicMillis,
) -> (RuntimeEvent, bool)
where
    L: ModelLoader,
{
    match command {
        RuntimeCommand::LoadModel {
            ticket,
            model_id,
            source,
            device,
            device_kind,
        } => (
            RuntimeEvent::ModelLoaded {
                ticket,
                result: runtime.load_model(model_id, &source, device, device_kind),
            },
            false,
        ),
        RuntimeCommand::StartRequest {
            ticket,
            handle,
            request_id,
            sequence_id,
            configuration,
        } => (
            RuntimeEvent::RequestStarted {
                ticket,
                result: runtime.start_request(handle, request_id, sequence_id, configuration),
            },
            false,
        ),
        RuntimeCommand::Prefill {
            ticket,
            request_id,
            tokens,
            emit_logits,
            logits,
        } => dispatch_prefill(runtime, ticket, request_id, &tokens, emit_logits, logits),
        RuntimeCommand::Decode {
            ticket,
            request_id,
            token,
            logits,
        } => dispatch_decode(runtime, ticket, request_id, token, logits),
        RuntimeCommand::CompleteRequest {
            ticket,
            request_id,
            reason,
        } => (
            RuntimeEvent::RequestFinished {
                ticket,
                request_id,
                result: runtime.complete_request(request_id, reason),
            },
            false,
        ),
        RuntimeCommand::CancelRequest {
            ticket,
            request_id,
            reason,
        } => (
            RuntimeEvent::RequestFinished {
                ticket,
                request_id,
                result: runtime.cancel_request(request_id, reason),
            },
            false,
        ),
        RuntimeCommand::UnloadModel {
            ticket,
            handle,
            policy,
        } => (
            RuntimeEvent::ModelUnload {
                ticket,
                result: runtime.unload_model(handle, policy, now),
            },
            false,
        ),
        RuntimeCommand::Snapshot { ticket } => (
            RuntimeEvent::Snapshot {
                ticket,
                runtime: runtime.snapshot(),
                models: runtime.model_snapshots(),
            },
            false,
        ),
        RuntimeCommand::Shutdown { ticket } => {
            let result = runtime.shutdown();
            let should_stop = result.is_ok();
            (RuntimeEvent::Shutdown { ticket, result }, should_stop)
        }
    }
}

fn dispatch_prefill<L>(
    runtime: &mut InferenceRuntime<L>,
    ticket: CommandTicket,
    request_id: domain_contracts::RequestId,
    tokens: &[domain_contracts::TokenId],
    emit_logits: bool,
    mut logits: Vec<f32>,
) -> (RuntimeEvent, bool)
where
    L: ModelLoader,
{
    let result = runtime.prefill(request_id, tokens, emit_logits, logits.as_mut_slice());
    (
        RuntimeEvent::PrefillCompleted {
            ticket,
            request_id,
            result,
            logits,
        },
        false,
    )
}

fn dispatch_decode<L>(
    runtime: &mut InferenceRuntime<L>,
    ticket: CommandTicket,
    request_id: domain_contracts::RequestId,
    token: domain_contracts::TokenId,
    mut logits: Vec<f32>,
) -> (RuntimeEvent, bool)
where
    L: ModelLoader,
{
    let result = runtime.decode(request_id, token, logits.as_mut_slice());
    (
        RuntimeEvent::DecodeCompleted {
            ticket,
            request_id,
            result,
            logits,
        },
        false,
    )
}

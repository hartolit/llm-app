//! Synchronous single-owner registry used directly or through the hosted worker.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use domain_contracts::{
    BackendSequence, CancellationReason, CancellationStatus, CapacityExhausted, CapacityResource,
    DecodeBuffers, DecodeInput, DecodeOutcome, DeviceId, DeviceKind, FinishReason, GenerationUsage,
    LifecycleAction, LoadConfiguration, LoadedModel, MemoryBudget, MemoryFootprint,
    ModelGeneration, ModelHandle, ModelId, ModelLifecycle, ModelLifecycleState, ModelLoader,
    MonotonicMillis, PrefillBuffers, PrefillInput, PrefillOutcome, RequestId,
    SequenceConfiguration, SequenceId, TokenId, UnloadPolicy, decode_checked, prefill_checked,
};

use crate::{
    DecodeReceipt, LoadReceipt, MemoryKind, ModelSnapshot, PrefillReceipt, RequestStartReceipt,
    RuntimeError, RuntimeLimits, RuntimeSnapshot, ShutdownReceipt, UnloadReceipt, UnloadStatus,
};

/// Synchronous inference registry with exclusive ownership of every loaded model.
pub struct InferenceRuntime<L>
where
    L: ModelLoader,
{
    loader: L,
    limits: RuntimeLimits,
    models: BTreeMap<ModelId, ModelSlot<L::Model>>,
    request_index: BTreeMap<RequestId, ModelId>,
    sequence_index: BTreeMap<SequenceId, RequestId>,
    generations: BTreeMap<ModelId, ModelGeneration>,
    reserved_footprint: MemoryFootprint,
    active_requests: u32,
    shutting_down: bool,
}

struct ModelSlot<M>
where
    M: LoadedModel,
{
    handle: ModelHandle,
    descriptor: domain_contracts::ModelDescriptor,
    lifecycle: ModelLifecycle,
    model: M,
    model_footprint: MemoryFootprint,
    reserved_footprint: MemoryFootprint,
    requests: BTreeMap<RequestId, RequestSlot<M::Sequence>>,
    cancelled_requests_during_unload: u32,
}

struct RequestSlot<S>
where
    S: BackendSequence,
{
    sequence: S,
    footprint: MemoryFootprint,
    usage: GenerationUsage,
}

impl<L> InferenceRuntime<L>
where
    L: ModelLoader,
{
    /// Creates an empty registry around one concrete backend loader.
    #[must_use]
    pub fn new(loader: L, limits: RuntimeLimits) -> Self {
        Self {
            loader,
            limits,
            models: BTreeMap::new(),
            request_index: BTreeMap::new(),
            sequence_index: BTreeMap::new(),
            generations: BTreeMap::new(),
            reserved_footprint: MemoryFootprint::default(),
            active_requests: 0,
            shutting_down: false,
        }
    }

    /// Returns immutable aggregate runtime state.
    #[must_use]
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            loaded_models: saturating_u32(self.models.len()),
            active_requests: self.active_requests,
            reserved_footprint: self.reserved_footprint,
            shutting_down: self.shutting_down,
        }
    }

    pub(crate) fn model_lifecycle_state(&self, model_id: ModelId) -> Option<ModelLifecycleState> {
        self.models
            .get(&model_id)
            .map(|slot| slot.lifecycle.state())
    }

    /// Collects per-model snapshots at a cold inspection boundary.
    #[must_use]
    pub fn model_snapshots(&self) -> Vec<ModelSnapshot> {
        self.models
            .values()
            .map(|slot| ModelSnapshot {
                handle: slot.handle,
                lifecycle: slot.lifecycle.state(),
                descriptor: slot.descriptor,
                reserved_footprint: slot.reserved_footprint,
                active_requests: saturating_u32(slot.requests.len()),
            })
            .collect()
    }

    /// Inspects, admits, and loads one model synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown has started; the model identity is already loaded;
    /// a model, generation, or memory limit is exceeded; a lifecycle transition fails;
    /// or the backend cannot plan or load a model that satisfies its declared contract.
    pub fn load_model(
        &mut self,
        model_id: ModelId,
        source: &L::Source,
        device: DeviceId,
        device_kind: DeviceKind,
    ) -> Result<LoadReceipt, RuntimeError> {
        self.reject_if_shutting_down()?;
        if self.models.contains_key(&model_id) {
            return Err(RuntimeError::ModelAlreadyLoaded(model_id));
        }
        if self.models.len() >= self.limits.maximum_loaded_models.get() as usize {
            return Err(RuntimeError::LoadedModelLimit {
                required: saturating_u32(self.models.len()).saturating_add(1),
                available: self.limits.maximum_loaded_models.get(),
            });
        }

        let handle = self.next_handle(model_id)?;
        let remaining_budget = remaining_budget(self.limits.memory_budget, self.reserved_footprint);
        let configuration = LoadConfiguration {
            handle,
            device,
            device_kind,
            memory_budget: remaining_budget,
        };
        let mut lifecycle = ModelLifecycle::new();
        lifecycle.begin_load()?;
        let plan = self.loader.plan_load(source, &configuration)?;
        let next_reserved = admit_footprint(
            self.reserved_footprint,
            plan.expected_footprint,
            self.limits.memory_budget,
        )?;
        let model = self.loader.load(source, &configuration)?;
        if model.handle() != handle || model.metadata() != &plan.descriptor.metadata {
            return Err(RuntimeError::BackendContractViolation);
        }
        lifecycle.complete_load()?;

        let slot = ModelSlot {
            handle,
            descriptor: plan.descriptor,
            lifecycle,
            model,
            model_footprint: plan.expected_footprint,
            reserved_footprint: plan.expected_footprint,
            requests: BTreeMap::new(),
            cancelled_requests_during_unload: 0,
        };
        match self.models.entry(model_id) {
            Entry::Vacant(entry) => {
                entry.insert(slot);
            }
            Entry::Occupied(_) => return Err(RuntimeError::ModelAlreadyLoaded(model_id)),
        }
        self.generations.insert(model_id, handle.generation);
        self.reserved_footprint = next_reserved;

        Ok(LoadReceipt {
            handle,
            descriptor: plan.descriptor,
            reserved_footprint: plan.expected_footprint,
        })
    }

    /// Creates one independently owned backend sequence for a request.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown has started; a request or sequence identity is
    /// already active; a runtime, model, or memory capacity is exceeded; the model
    /// handle or lifecycle is invalid; or the backend cannot plan or create the sequence.
    pub fn start_request(
        &mut self,
        handle: ModelHandle,
        request_id: RequestId,
        sequence_id: SequenceId,
        configuration: SequenceConfiguration,
    ) -> Result<RequestStartReceipt, RuntimeError> {
        self.reject_if_shutting_down()?;
        if self.request_index.contains_key(&request_id) {
            return Err(RuntimeError::RequestAlreadyActive(request_id));
        }
        if self.sequence_index.contains_key(&sequence_id) {
            return Err(RuntimeError::SequenceAlreadyActive(sequence_id));
        }
        if self.active_requests >= self.limits.maximum_active_requests.get() {
            return Err(RuntimeError::CapacityExhausted(CapacityExhausted::new(
                CapacityResource::ActiveRequests,
                u64::from(self.active_requests).saturating_add(1),
                u64::from(self.limits.maximum_active_requests.get()),
            )));
        }

        let current_reserved = self.reserved_footprint;
        let memory_budget = self.limits.memory_budget;
        let slot = self.exact_model_mut(handle)?;
        match slot.lifecycle.state() {
            ModelLifecycleState::Ready | ModelLifecycleState::Active { .. } => {}
            _ => {
                return Err(RuntimeError::Lifecycle(
                    domain_contracts::LifecycleError::InvalidTransition,
                ));
            }
        }
        if slot.requests.len() >= slot.descriptor.capabilities.maximum_sequences as usize {
            return Err(RuntimeError::CapacityExhausted(CapacityExhausted::new(
                CapacityResource::ActiveSequences,
                saturating_u64(slot.requests.len()).saturating_add(1),
                u64::from(slot.descriptor.capabilities.maximum_sequences),
            )));
        }

        let plan = slot.model.plan_sequence(&configuration)?;
        let next_reserved =
            admit_footprint(current_reserved, plan.expected_footprint, memory_budget)?;
        let next_slot_reserved =
            checked_add_footprint(slot.reserved_footprint, plan.expected_footprint)?;
        let sequence = slot.model.create_sequence(sequence_id, &configuration)?;
        if sequence.id() != sequence_id {
            return Err(RuntimeError::BackendContractViolation);
        }
        slot.lifecycle.start_request()?;
        let request = RequestSlot {
            sequence,
            footprint: plan.expected_footprint,
            usage: GenerationUsage::default(),
        };
        match slot.requests.entry(request_id) {
            Entry::Vacant(entry) => {
                entry.insert(request);
            }
            Entry::Occupied(_) => {
                slot.lifecycle.finish_request()?;
                return Err(RuntimeError::RequestAlreadyActive(request_id));
            }
        }
        slot.reserved_footprint = next_slot_reserved;

        match self.request_index.entry(request_id) {
            Entry::Vacant(entry) => {
                entry.insert(handle.id);
            }
            Entry::Occupied(_) => return Err(RuntimeError::BackendContractViolation),
        }
        match self.sequence_index.entry(sequence_id) {
            Entry::Vacant(entry) => {
                entry.insert(request_id);
            }
            Entry::Occupied(_) => return Err(RuntimeError::BackendContractViolation),
        }
        self.active_requests = self.active_requests.saturating_add(1);
        self.reserved_footprint = next_reserved;

        Ok(RequestStartReceipt {
            request_id,
            sequence_id,
            logits_capacity: plan.logits_capacity,
            reserved_footprint: plan.expected_footprint,
        })
    }

    /// Executes one checked prompt-prefill operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the request or its model is no longer active, the checked
    /// backend operation fails, or destroying a finished or failed sequence violates
    /// a backend, lifecycle, or memory-accounting invariant.
    pub fn prefill(
        &mut self,
        request_id: RequestId,
        tokens: &[TokenId],
        emit_logits: bool,
        logits: &mut [f32],
    ) -> Result<PrefillReceipt, RuntimeError> {
        let model_id = self.request_model_id(request_id)?;
        let operation = {
            let slot = self
                .models
                .get_mut(&model_id)
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            let request = slot
                .requests
                .get_mut(&request_id)
                .ok_or(RuntimeError::RequestNotActive(request_id))?;
            let outcome = prefill_checked(
                &mut slot.model,
                &mut request.sequence,
                PrefillInput::new(tokens, emit_logits),
                PrefillBuffers::new(logits),
                CancellationStatus::Running,
            );
            match outcome {
                Ok(PrefillOutcome::Ready {
                    consumed_tokens,
                    position,
                    logits_written,
                }) => {
                    request.usage.prompt_tokens = request
                        .usage
                        .prompt_tokens
                        .saturating_add(saturating_u64(consumed_tokens));
                    Ok((
                        PrefillOutcome::Ready {
                            consumed_tokens,
                            position,
                            logits_written,
                        },
                        request.usage,
                    ))
                }
                Ok(PrefillOutcome::Finished(reason)) => {
                    Ok((PrefillOutcome::Finished(reason), request.usage))
                }
                Err(error) => Err(error),
            }
        };

        match operation {
            Ok((outcome, usage)) => {
                if matches!(outcome, PrefillOutcome::Finished(_)) {
                    self.remove_request(request_id)?;
                }
                Ok(PrefillReceipt { outcome, usage })
            }
            Err(error) => {
                self.remove_request(request_id)?;
                Err(RuntimeError::Sequence(error))
            }
        }
    }

    /// Executes one checked incremental decode operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the request or its model is no longer active, the checked
    /// backend operation fails, or destroying a finished or failed sequence violates
    /// a backend, lifecycle, or memory-accounting invariant.
    pub fn decode(
        &mut self,
        request_id: RequestId,
        token: TokenId,
        logits: &mut [f32],
    ) -> Result<DecodeReceipt, RuntimeError> {
        let model_id = self.request_model_id(request_id)?;
        let operation = {
            let slot = self
                .models
                .get_mut(&model_id)
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            let request = slot
                .requests
                .get_mut(&request_id)
                .ok_or(RuntimeError::RequestNotActive(request_id))?;
            let outcome = decode_checked(
                &mut slot.model,
                &mut request.sequence,
                DecodeInput::new(token),
                DecodeBuffers::new(logits),
                CancellationStatus::Running,
            );
            match outcome {
                Ok(DecodeOutcome::Ready {
                    position,
                    logits_written,
                }) => {
                    request.usage.generated_tokens =
                        request.usage.generated_tokens.saturating_add(1);
                    Ok((
                        DecodeOutcome::Ready {
                            position,
                            logits_written,
                        },
                        request.usage,
                    ))
                }
                Ok(DecodeOutcome::Finished(reason)) => {
                    Ok((DecodeOutcome::Finished(reason), request.usage))
                }
                Err(error) => Err(error),
            }
        };

        match operation {
            Ok((outcome, usage)) => {
                if matches!(outcome, DecodeOutcome::Finished(_)) {
                    self.remove_request(request_id)?;
                }
                Ok(DecodeReceipt { outcome, usage })
            }
            Err(error) => {
                self.remove_request(request_id)?;
                Err(RuntimeError::Sequence(error))
            }
        }
    }

    /// Completes one request and drops its sequence at a safe boundary.
    ///
    /// # Errors
    ///
    /// Returns an error if the request or model is no longer active, sequence destruction
    /// fails, or removing the request violates a lifecycle or memory-accounting invariant.
    pub fn complete_request(
        &mut self,
        request_id: RequestId,
        reason: FinishReason,
    ) -> Result<FinishReason, RuntimeError> {
        self.remove_request(request_id)?;
        Ok(reason)
    }

    /// Cancels one request and drops its sequence at a safe boundary.
    ///
    /// # Errors
    ///
    /// Returns an error if the request or model is no longer active, sequence destruction
    /// fails, or removing the request violates a lifecycle or memory-accounting invariant.
    pub fn cancel_request(
        &mut self,
        request_id: RequestId,
        reason: CancellationReason,
    ) -> Result<FinishReason, RuntimeError> {
        self.remove_request(request_id)?;
        Ok(FinishReason::Cancelled(reason))
    }

    /// Applies one explicit unload policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is unknown or stale, the unload lifecycle transition
    /// is invalid, an active sequence cannot be destroyed, model unload preparation fails,
    /// or releasing model resources violates runtime accounting invariants.
    pub fn unload_model(
        &mut self,
        handle: ModelHandle,
        policy: UnloadPolicy,
        now: MonotonicMillis,
    ) -> Result<UnloadReceipt, RuntimeError> {
        if !self.models.contains_key(&handle.id) {
            return self.absent_unload_receipt(handle);
        }
        let action = {
            let slot = self.exact_model_mut(handle)?;
            slot.lifecycle.request_unload(policy, now)?
        };

        match action {
            LifecycleAction::None => Ok(UnloadReceipt {
                handle,
                status: UnloadStatus::Draining,
                cancelled_requests: 0,
            }),
            LifecycleAction::CancelActive { .. } => {
                let cancelled_requests = self.cancel_all_requests(handle.id)?;
                self.release_model(handle.id)?;
                Ok(UnloadReceipt {
                    handle,
                    status: UnloadStatus::Unloaded,
                    cancelled_requests,
                })
            }
            LifecycleAction::ReleaseModel => {
                self.release_model(handle.id)?;
                Ok(UnloadReceipt {
                    handle,
                    status: UnloadStatus::Unloaded,
                    cancelled_requests: 0,
                })
            }
            LifecycleAction::UnloadComplete => Ok(UnloadReceipt {
                handle,
                status: UnloadStatus::AlreadyAbsent,
                cancelled_requests: 0,
            }),
        }
    }

    /// Enforces at most one timeout-driven or pending-unload transition.
    ///
    /// Calling this method at the configured host polling cadence guarantees that
    /// an expired drain window escalates without depending on event-consumer speed.
    ///
    /// # Errors
    ///
    /// Returns an error if a pending lifecycle transition fails, an active sequence
    /// cannot be destroyed, model unload preparation fails, or resource accounting
    /// invariants are violated while completing an unload.
    pub fn poll(&mut self, now: MonotonicMillis) -> Result<bool, RuntimeError> {
        match self.poll_unload_transition(now) {
            Some((_, Ok(_))) => Ok(true),
            Some((_, Err(error))) => Err(error),
            None => Ok(false),
        }
    }

    pub(crate) fn poll_unload_transition(
        &mut self,
        now: MonotonicMillis,
    ) -> Option<(ModelHandle, Result<UnloadReceipt, RuntimeError>)> {
        let mut expired = None;
        for (model_id, slot) in &mut self.models {
            if matches!(slot.lifecycle.state(), ModelLifecycleState::Draining { .. }) {
                match slot.lifecycle.poll(now) {
                    Ok(LifecycleAction::CancelActive { .. }) => {
                        expired = Some((*model_id, slot.handle));
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Some((slot.handle, Err(error.into())));
                    }
                }
            }
        }
        if let Some((model_id, handle)) = expired {
            let result = self
                .cancel_all_requests(model_id)
                .and_then(|cancelled_requests| {
                    self.release_model(model_id)?;
                    Ok(UnloadReceipt {
                        handle,
                        status: UnloadStatus::Unloaded,
                        cancelled_requests,
                    })
                });
            return Some((handle, result));
        }

        let pending_cancellation = self.models.iter().find_map(|(model_id, slot)| {
            if matches!(
                slot.lifecycle.state(),
                ModelLifecycleState::Cancelling { .. }
            ) {
                Some((*model_id, slot.handle))
            } else {
                None
            }
        });
        if let Some((model_id, handle)) = pending_cancellation {
            let result = self
                .cancel_all_requests(model_id)
                .and_then(|cancelled_requests| {
                    self.release_model(model_id)?;
                    Ok(UnloadReceipt {
                        handle,
                        status: UnloadStatus::Unloaded,
                        cancelled_requests,
                    })
                });
            return Some((handle, result));
        }

        let pending_unload = self.models.iter().find_map(|(model_id, slot)| {
            if slot.lifecycle.state() == ModelLifecycleState::Unloading {
                Some((*model_id, slot.handle))
            } else {
                None
            }
        });
        if let Some((model_id, handle)) = pending_unload {
            let result = self.release_model(model_id).map(|()| UnloadReceipt {
                handle,
                status: UnloadStatus::Unloaded,
                cancelled_requests: 0,
            });
            return Some((handle, result));
        }
        None
    }

    /// Cancels every request and unloads every resident model.
    ///
    /// # Errors
    ///
    /// Returns an error if resident state is inconsistent, a lifecycle transition or
    /// sequence destruction fails, model unload preparation fails, or releasing request
    /// or model resources violates runtime accounting invariants.
    pub fn shutdown(&mut self) -> Result<ShutdownReceipt, RuntimeError> {
        self.shutting_down = true;
        let mut unloaded_models = 0_u32;
        let mut cancelled_requests = 0_u32;

        while let Some((&model_id, _)) = self.models.first_key_value() {
            let state = self
                .models
                .get(&model_id)
                .map(|slot| slot.lifecycle.state())
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            match state {
                ModelLifecycleState::Ready => {
                    let action = self
                        .models
                        .get_mut(&model_id)
                        .ok_or(RuntimeError::ModelNotLoaded(model_id))?
                        .lifecycle
                        .request_unload(UnloadPolicy::CancelActive, MonotonicMillis::new(0))?;
                    if action != LifecycleAction::ReleaseModel {
                        return Err(RuntimeError::BackendContractViolation);
                    }
                }
                ModelLifecycleState::Active { .. } => {
                    self.models
                        .get_mut(&model_id)
                        .ok_or(RuntimeError::ModelNotLoaded(model_id))?
                        .lifecycle
                        .request_unload(UnloadPolicy::CancelActive, MonotonicMillis::new(0))?;
                    cancelled_requests =
                        cancelled_requests.saturating_add(self.cancel_all_requests(model_id)?);
                }
                ModelLifecycleState::Draining { .. } | ModelLifecycleState::Cancelling { .. } => {
                    cancelled_requests =
                        cancelled_requests.saturating_add(self.cancel_all_requests(model_id)?);
                }
                ModelLifecycleState::Unloading => {}
                ModelLifecycleState::Absent
                | ModelLifecycleState::Loading
                | ModelLifecycleState::Failed { .. } => {
                    return Err(RuntimeError::Lifecycle(
                        domain_contracts::LifecycleError::InvalidTransition,
                    ));
                }
            }
            self.release_model(model_id)?;
            unloaded_models = unloaded_models.saturating_add(1);
        }

        Ok(ShutdownReceipt {
            unloaded_models,
            cancelled_requests,
        })
    }

    const fn reject_if_shutting_down(&self) -> Result<(), RuntimeError> {
        if self.shutting_down {
            Err(RuntimeError::ShuttingDown)
        } else {
            Ok(())
        }
    }

    fn next_handle(&self, model_id: ModelId) -> Result<ModelHandle, RuntimeError> {
        let current = self
            .generations
            .get(&model_id)
            .map_or(0, |value| value.get());
        let next = current
            .checked_add(1)
            .ok_or(RuntimeError::ModelGenerationExhausted(model_id))?;
        Ok(ModelHandle::new(model_id, ModelGeneration::new(next)))
    }

    fn exact_model_mut(
        &mut self,
        handle: ModelHandle,
    ) -> Result<&mut ModelSlot<L::Model>, RuntimeError> {
        let slot = self
            .models
            .get_mut(&handle.id)
            .ok_or(RuntimeError::ModelNotLoaded(handle.id))?;
        if slot.handle != handle {
            return Err(RuntimeError::StaleModelHandle {
                provided: handle,
                current: slot.handle,
            });
        }
        Ok(slot)
    }

    fn request_model_id(&self, request_id: RequestId) -> Result<ModelId, RuntimeError> {
        self.request_index
            .get(&request_id)
            .copied()
            .ok_or(RuntimeError::RequestNotActive(request_id))
    }

    fn remove_request(&mut self, request_id: RequestId) -> Result<(), RuntimeError> {
        let model_id = self.request_model_id(request_id)?;
        let (sequence_id, footprint, action) = {
            let slot = self
                .models
                .get_mut(&model_id)
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            {
                let request = slot
                    .requests
                    .get_mut(&request_id)
                    .ok_or(RuntimeError::RequestNotActive(request_id))?;
                slot.model
                    .destroy_sequence(&mut request.sequence)
                    .map_err(RuntimeError::Sequence)?;
            }
            let request = slot
                .requests
                .remove(&request_id)
                .ok_or(RuntimeError::BackendContractViolation)?;
            let sequence_id = request.sequence.id();
            let footprint = request.footprint;
            slot.reserved_footprint = checked_sub_footprint(slot.reserved_footprint, footprint)?;
            let action = slot.lifecycle.finish_request()?;
            (sequence_id, footprint, action)
        };
        self.request_index.remove(&request_id);
        self.sequence_index.remove(&sequence_id);
        self.active_requests = self
            .active_requests
            .checked_sub(1)
            .ok_or(RuntimeError::BackendContractViolation)?;
        self.reserved_footprint = checked_sub_footprint(self.reserved_footprint, footprint)?;

        if action == LifecycleAction::ReleaseModel {
            self.release_model(model_id)?;
        }
        Ok(())
    }

    fn cancel_all_requests(&mut self, model_id: ModelId) -> Result<u32, RuntimeError> {
        loop {
            let removal = {
                let slot = self
                    .models
                    .get_mut(&model_id)
                    .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
                let Some(request_id) = slot
                    .requests
                    .first_key_value()
                    .map(|(request_id, _)| *request_id)
                else {
                    break;
                };
                {
                    let request = slot
                        .requests
                        .get_mut(&request_id)
                        .ok_or(RuntimeError::BackendContractViolation)?;
                    slot.model
                        .destroy_sequence(&mut request.sequence)
                        .map_err(RuntimeError::Sequence)?;
                }
                let request = slot
                    .requests
                    .remove(&request_id)
                    .ok_or(RuntimeError::BackendContractViolation)?;
                let sequence_id = request.sequence.id();
                let footprint = request.footprint;
                slot.reserved_footprint =
                    checked_sub_footprint(slot.reserved_footprint, footprint)?;
                slot.lifecycle.finish_request()?;
                (request_id, sequence_id, footprint)
            };

            let (request_id, sequence_id, footprint) = removal;
            self.request_index.remove(&request_id);
            self.sequence_index.remove(&sequence_id);
            self.active_requests = self
                .active_requests
                .checked_sub(1)
                .ok_or(RuntimeError::BackendContractViolation)?;
            self.reserved_footprint = checked_sub_footprint(self.reserved_footprint, footprint)?;
            let slot = self
                .models
                .get_mut(&model_id)
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            slot.cancelled_requests_during_unload =
                slot.cancelled_requests_during_unload.saturating_add(1);
        }
        self.models
            .get(&model_id)
            .map(|slot| slot.cancelled_requests_during_unload)
            .ok_or(RuntimeError::ModelNotLoaded(model_id))
    }

    fn release_model(&mut self, model_id: ModelId) -> Result<(), RuntimeError> {
        {
            let slot = self
                .models
                .get_mut(&model_id)
                .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
            if !slot.requests.is_empty() || slot.lifecycle.state() != ModelLifecycleState::Unloading
            {
                return Err(RuntimeError::Lifecycle(
                    domain_contracts::LifecycleError::InvalidTransition,
                ));
            }
            slot.model.prepare_unload()?;
            slot.lifecycle.complete_unload()?;
        }
        let slot = self
            .models
            .remove(&model_id)
            .ok_or(RuntimeError::ModelNotLoaded(model_id))?;
        self.reserved_footprint =
            checked_sub_footprint(self.reserved_footprint, slot.model_footprint)?;
        Ok(())
    }

    fn absent_unload_receipt(&self, handle: ModelHandle) -> Result<UnloadReceipt, RuntimeError> {
        let Some(generation) = self.generations.get(&handle.id).copied() else {
            return Err(RuntimeError::ModelNotLoaded(handle.id));
        };
        let current = ModelHandle::new(handle.id, generation);
        if current != handle {
            return Err(RuntimeError::StaleModelHandle {
                provided: handle,
                current,
            });
        }
        Ok(UnloadReceipt {
            handle,
            status: UnloadStatus::AlreadyAbsent,
            cancelled_requests: 0,
        })
    }
}

const fn remaining_budget(limit: MemoryBudget, used: MemoryFootprint) -> MemoryBudget {
    MemoryBudget {
        host_bytes: limit.host_bytes.saturating_sub(used.host_bytes()),
        device_bytes: limit.device_bytes.saturating_sub(used.device_bytes()),
    }
}

fn admit_footprint(
    current: MemoryFootprint,
    additional: MemoryFootprint,
    budget: MemoryBudget,
) -> Result<MemoryFootprint, RuntimeError> {
    let next = checked_add_footprint(current, additional)?;
    let required_host = next.host_bytes();
    if required_host > budget.host_bytes {
        return Err(RuntimeError::InsufficientMemory {
            kind: MemoryKind::Host,
            required_bytes: required_host,
            available_bytes: budget.host_bytes,
        });
    }
    let required_device = next.device_bytes();
    if required_device > budget.device_bytes {
        return Err(RuntimeError::InsufficientMemory {
            kind: MemoryKind::Device,
            required_bytes: required_device,
            available_bytes: budget.device_bytes,
        });
    }
    Ok(next)
}

fn checked_add_footprint(
    left: MemoryFootprint,
    right: MemoryFootprint,
) -> Result<MemoryFootprint, RuntimeError> {
    Ok(MemoryFootprint {
        host_weight_bytes: left
            .host_weight_bytes
            .checked_add(right.host_weight_bytes)
            .ok_or(RuntimeError::MemoryArithmeticOverflow)?,
        device_weight_bytes: left
            .device_weight_bytes
            .checked_add(right.device_weight_bytes)
            .ok_or(RuntimeError::MemoryArithmeticOverflow)?,
        host_working_bytes: left
            .host_working_bytes
            .checked_add(right.host_working_bytes)
            .ok_or(RuntimeError::MemoryArithmeticOverflow)?,
        device_working_bytes: left
            .device_working_bytes
            .checked_add(right.device_working_bytes)
            .ok_or(RuntimeError::MemoryArithmeticOverflow)?,
        cache_bytes_per_token: left
            .cache_bytes_per_token
            .checked_add(right.cache_bytes_per_token)
            .ok_or(RuntimeError::MemoryArithmeticOverflow)?,
    })
}

fn checked_sub_footprint(
    left: MemoryFootprint,
    right: MemoryFootprint,
) -> Result<MemoryFootprint, RuntimeError> {
    Ok(MemoryFootprint {
        host_weight_bytes: left
            .host_weight_bytes
            .checked_sub(right.host_weight_bytes)
            .ok_or(RuntimeError::MemoryArithmeticUnderflow)?,
        device_weight_bytes: left
            .device_weight_bytes
            .checked_sub(right.device_weight_bytes)
            .ok_or(RuntimeError::MemoryArithmeticUnderflow)?,
        host_working_bytes: left
            .host_working_bytes
            .checked_sub(right.host_working_bytes)
            .ok_or(RuntimeError::MemoryArithmeticUnderflow)?,
        device_working_bytes: left
            .device_working_bytes
            .checked_sub(right.device_working_bytes)
            .ok_or(RuntimeError::MemoryArithmeticUnderflow)?,
        cache_bytes_per_token: left
            .cache_bytes_per_token
            .checked_sub(right.cache_bytes_per_token)
            .ok_or(RuntimeError::MemoryArithmeticUnderflow)?,
    })
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

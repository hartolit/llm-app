//! Bounded cooperative shutdown for application-owned host workers.

use std::time::{Duration, Instant};

use host_runtime::{HostThread, SendTimeoutError, yield_now};
use inference_runtime::{RuntimeCommand, RuntimeEvent, RuntimeThread};

use crate::hub_worker::HubCommand;
use crate::support::thread_failure;
use crate::{
    ApplicationActivity, ApplicationError, ApplicationFailure, ApplicationFailureKind,
    ApplicationRuntime, ApplicationWorker,
};

pub fn shutdown(runtime: &mut ApplicationRuntime) -> Result<(), ApplicationError> {
    if runtime.state.activity() == ApplicationActivity::ShuttingDown {
        return Ok(());
    }
    runtime.state.begin_shutdown();

    let mut first_error = request_hub_shutdown(runtime).err();
    record_first_error(&mut first_error, shutdown_runtime(runtime).err());
    record_first_error(&mut first_error, join_runtime_worker(runtime).err());
    record_first_error(&mut first_error, join_hub_worker(runtime).err());

    first_error.map_or(Ok(()), Err)
}

fn request_hub_shutdown(runtime: &mut ApplicationRuntime) -> Result<(), ApplicationError> {
    if !runtime.state.hub_available() {
        return Ok(());
    }
    match runtime.hub_commands.send_timeout(
        HubCommand::Shutdown,
        runtime.configuration.timing.hub_command_shutdown_timeout,
    ) {
        Ok(()) | Err(SendTimeoutError::Disconnected(_)) => {
            runtime.state.disconnect_hub();
            Ok(())
        }
        Err(SendTimeoutError::Timeout(_)) => Err(ApplicationError::HubBusy),
    }
}

fn shutdown_runtime(runtime: &mut ApplicationRuntime) -> Result<(), ApplicationError> {
    if !runtime.state.inference_available() {
        return Ok(());
    }
    while runtime.inference.try_receive().is_ok() {}
    let ticket = runtime.next_ticket()?;
    let deadline = Instant::now() + runtime.configuration.timing.runtime_shutdown_timeout;
    let mut pending = RuntimeCommand::Shutdown { ticket };
    loop {
        match runtime.inference.try_submit(pending) {
            Ok(()) => break,
            Err(inference_runtime::RuntimeSubmitError::Disconnected(_)) => {
                runtime.state.disconnect_inference();
                return Ok(());
            }
            Err(inference_runtime::RuntimeSubmitError::Full(command)) => {
                if Instant::now() >= deadline {
                    return Err(ApplicationError::RuntimeBusy);
                }
                pending = command;
                yield_now();
            }
        }
    }

    loop {
        if Instant::now() >= deadline {
            return Err(ApplicationError::ShutdownTimeout(
                ApplicationWorker::Inference,
            ));
        }
        match runtime
            .inference
            .receive_timeout(runtime.configuration.timing.runtime_shutdown_event_poll)
        {
            Ok(RuntimeEvent::Shutdown {
                ticket: event_ticket,
                result,
            }) if event_ticket == ticket => {
                runtime.state.disconnect_inference();
                return result.map(|_| ()).map_err(|error| {
                    ApplicationFailure::from_debug(
                        ApplicationFailureKind::Inference,
                        "inference shutdown failed",
                        error,
                    )
                    .into()
                });
            }
            Ok(_) | Err(inference_runtime::RuntimeReceiveError::Timeout) => {}
            Err(inference_runtime::RuntimeReceiveError::Disconnected) => {
                runtime.state.disconnect_inference();
                return Ok(());
            }
        }
    }
}

fn join_runtime_worker(runtime: &mut ApplicationRuntime) -> Result<(), ApplicationError> {
    let Some(thread) = runtime.inference_thread.take() else {
        return Ok(());
    };
    wait_for_runtime_thread(
        &thread,
        runtime.configuration.timing.runtime_join_timeout,
        runtime.configuration.timing.runtime_join_poll,
    )?;
    thread.join().map_err(thread_failure)
}

fn join_hub_worker(runtime: &mut ApplicationRuntime) -> Result<(), ApplicationError> {
    let Some(thread) = runtime.hub_thread.take() else {
        return Ok(());
    };
    wait_for_host_thread(
        &thread,
        runtime.configuration.timing.hub_shutdown_timeout,
        runtime.configuration.timing.hub_shutdown_poll,
    )?;
    thread.join().map_err(thread_failure)
}

fn wait_for_runtime_thread(
    thread: &RuntimeThread,
    timeout: Duration,
    poll: Duration,
) -> Result<(), ApplicationError> {
    let deadline = Instant::now() + timeout;
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            return Err(ApplicationError::ShutdownTimeout(
                ApplicationWorker::Inference,
            ));
        }
        std::thread::sleep(poll);
    }
    Ok(())
}

fn wait_for_host_thread(
    thread: &HostThread<()>,
    timeout: Duration,
    poll: Duration,
) -> Result<(), ApplicationError> {
    let deadline = Instant::now() + timeout;
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            return Err(ApplicationError::ShutdownTimeout(ApplicationWorker::Hub));
        }
        std::thread::sleep(poll);
    }
    Ok(())
}

fn record_first_error(first: &mut Option<ApplicationError>, candidate: Option<ApplicationError>) {
    if first.is_none() {
        *first = candidate;
    }
}

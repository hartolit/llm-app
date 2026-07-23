//! Exclusive model ownership, admission control, cancellation, and bounded hosting.

#![forbid(unsafe_code)]

mod command;
mod configuration;
mod error;
mod generation;
mod runtime;
mod worker;

pub use command::{
    CommandTicket, DecodeReceipt, LoadReceipt, ModelSnapshot, PrefillReceipt, RequestStartReceipt,
    RuntimeCommand, RuntimeEvent, RuntimeSnapshot, ShutdownReceipt, UnloadReceipt, UnloadStatus,
};
pub use configuration::{HostedRuntimeConfiguration, RuntimeLimits};
pub use error::{
    CleanupFailureReport, FailureClass, MemoryKind, RuntimeError, RuntimeOperation,
    RuntimeReceiveError, RuntimeSubmitError, SamplingFailure,
};
pub use generation::{
    GenerationAdmission, GenerationOutcome, GenerationOutputState, GenerationRequest,
    GenerationStopSequence,
};
pub use runtime::InferenceRuntime;
pub use worker::{HostedRuntime, HostedRuntimeStartError, RuntimeThread, start_hosted_runtime};

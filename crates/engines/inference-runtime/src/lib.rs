//! Exclusive model ownership, admission control, cancellation, and bounded hosting.

#![forbid(unsafe_code)]

mod command;
mod configuration;
mod error;
mod runtime;
mod worker;

pub use command::{
    CommandTicket, DecodeReceipt, LoadReceipt, ModelSnapshot, PrefillReceipt, RequestStartReceipt,
    RuntimeCommand, RuntimeEvent, RuntimeSnapshot, ShutdownReceipt, UnloadReceipt, UnloadStatus,
};
pub use configuration::{HostedRuntimeConfiguration, RuntimeLimits};
pub use error::{MemoryKind, RuntimeError, RuntimeReceiveError, RuntimeSubmitError};
pub use runtime::InferenceRuntime;
pub use worker::{HostedRuntime, RuntimeThread, start_hosted_runtime};

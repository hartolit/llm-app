//! Stable runtime, admission, and host-transport failures.

use domain_contracts::{
    CapacityExhausted, LifecycleError, LoadError, ModelError, ModelHandle, ModelId, RequestId,
    SequenceError, SynchronizationError,
};

use core::fmt::{self, Debug, Formatter};

use crate::RuntimeCommand;

/// Memory domain whose aggregate budget was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryKind {
    /// Host-addressable memory.
    Host,
    /// Device-local memory.
    Device,
}

/// Inference registry or backend operation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The logical model identity is already resident.
    ModelAlreadyLoaded(ModelId),
    /// No resident model has the requested logical identity.
    ModelNotLoaded(ModelId),
    /// A retained handle addresses an older or otherwise different generation.
    StaleModelHandle {
        /// Handle supplied by the caller.
        provided: ModelHandle,
        /// Current resident or most recently completed generation.
        current: ModelHandle,
    },
    /// The request identity is already active.
    RequestAlreadyActive(RequestId),
    /// No active request has the supplied identity.
    RequestNotActive(RequestId),
    /// The sequence identity is already owned by another active request.
    SequenceAlreadyActive(domain_contracts::SequenceId),
    /// A model-generation counter could not be incremented.
    ModelGenerationExhausted(ModelId),
    /// Runtime shutdown has begun and new work is rejected.
    ShuttingDown,
    /// The configured resident-model count was exceeded.
    LoadedModelLimit {
        /// Model count required after the attempted admission.
        required: u32,
        /// Configured resident-model limit.
        available: u32,
    },
    /// Aggregate memory accounting overflowed its integer representation.
    MemoryArithmeticOverflow,
    /// Aggregate memory accounting underflowed, indicating an internal invariant failure.
    MemoryArithmeticUnderflow,
    /// A fixed registry or backend capacity was exhausted.
    CapacityExhausted(CapacityExhausted),
    /// Aggregate model or sequence memory admission failed.
    InsufficientMemory {
        /// Memory domain that exceeded its hard limit.
        kind: MemoryKind,
        /// Total resident bytes required after the attempted admission.
        required_bytes: u64,
        /// Configured aggregate byte limit.
        available_bytes: u64,
    },
    /// Model loading failed.
    Load(LoadError),
    /// Loaded-model operation failed.
    Model(ModelError),
    /// Sequence operation failed.
    Sequence(SequenceError),
    /// Synchronization or unload preparation failed.
    Synchronization(SynchronizationError),
    /// Lifecycle transition failed.
    Lifecycle(LifecycleError),
    /// Backend returned a handle or metadata inconsistent with its accepted plan.
    BackendContractViolation,
}

impl From<CapacityExhausted> for RuntimeError {
    fn from(value: CapacityExhausted) -> Self {
        Self::CapacityExhausted(value)
    }
}

impl From<LoadError> for RuntimeError {
    fn from(value: LoadError) -> Self {
        Self::Load(value)
    }
}

impl From<ModelError> for RuntimeError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}

impl From<SequenceError> for RuntimeError {
    fn from(value: SequenceError) -> Self {
        Self::Sequence(value)
    }
}

impl From<SynchronizationError> for RuntimeError {
    fn from(value: SynchronizationError) -> Self {
        Self::Synchronization(value)
    }
}

impl From<LifecycleError> for RuntimeError {
    fn from(value: LifecycleError) -> Self {
        Self::Lifecycle(value)
    }
}

/// Non-blocking submission failure retaining ownership of the command.
pub enum RuntimeSubmitError<S> {
    /// The bounded command queue is full.
    Full(RuntimeCommand<S>),
    /// The runtime worker has stopped.
    Disconnected(RuntimeCommand<S>),
}

/// Event receive failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeReceiveError {
    /// No event arrived before the requested timeout.
    Timeout,
    /// The runtime worker has stopped and no events remain.
    Disconnected,
}

impl<S> Debug for RuntimeSubmitError<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(_) => formatter.write_str("RuntimeSubmitError::Full(..)"),
            Self::Disconnected(_) => formatter.write_str("RuntimeSubmitError::Disconnected(..)"),
        }
    }
}

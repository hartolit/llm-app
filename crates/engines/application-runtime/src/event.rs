//! Structured application events consumed by Slint, Tauri, CLI, or other frontends.

use domain_contracts::ModelHandle;

use crate::{ApplicationFailure, LoadedModel, ResolvedModel};

/// Frontend-neutral result of polling the application orchestrator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationEvent {
    /// Immutable model artifacts and tokenizer were resolved successfully.
    ModelResolved {
        /// Validated immutable model summary.
        model: ResolvedModel,
        /// Non-fatal catalogue persistence failure, when persistence failed.
        persistence_warning: Option<ApplicationFailure>,
    },
    /// Artifact resolution or tokenizer validation failed.
    ModelResolutionFailed {
        /// Normalized failure.
        failure: ApplicationFailure,
    },
    /// A model generation was loaded successfully.
    ModelLoaded {
        /// Loaded model summary.
        model: LoadedModel,
    },
    /// Loading failed before a safe resident model became available.
    ModelLoadFailed {
        /// Normalized failure.
        failure: ApplicationFailure,
    },
    /// Tokenizer and model metadata were incompatible and unload was requested.
    ModelCompatibilityFailed {
        /// Normalized compatibility diagnostic.
        failure: ApplicationFailure,
    },
    /// Active work is draining before deterministic unload.
    ModelDraining {
        /// Generation being drained.
        handle: ModelHandle,
    },
    /// Model resources are no longer resident.
    ModelUnloaded {
        /// Generation released or confirmed absent.
        handle: ModelHandle,
        /// Requests force-cancelled at safe boundaries.
        cancelled_requests: u32,
    },
    /// Unloading failed and the frontend may retry.
    ModelUnloadFailed {
        /// Normalized failure.
        failure: ApplicationFailure,
    },
    /// Hub worker disconnected and cannot accept further resolution requests.
    HubDisconnected,
    /// Inference worker disconnected and cannot accept further model operations.
    RuntimeDisconnected,
}

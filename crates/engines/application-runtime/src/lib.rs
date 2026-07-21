//! Frontend-neutral orchestration for local model acquisition and lifecycle.

#![forbid(unsafe_code)]

mod configuration;
mod error;
mod event;
mod hub_worker;
mod runtime;
mod shutdown;
mod state;
mod support;

pub use configuration::{
    ApplicationHubConfiguration, ApplicationPreferences, ApplicationRuntimeConfiguration,
    ApplicationTiming,
};
pub use domain_contracts::ScalarType;
pub use error::{
    ApplicationConfigurationField, ApplicationError, ApplicationFailure, ApplicationFailureKind,
    ApplicationWorker,
};
pub use event::ApplicationEvent;
pub use runtime::ApplicationRuntime;
pub use state::{ApplicationActivity, ApplicationState, LoadedModel, ResolvedModel};

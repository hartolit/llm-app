//! CPU GGUF backend implemented through safe llama.cpp bindings.

#![deny(unsafe_code)]

mod failure;
mod loader;
mod metadata;
mod model;
mod source;

pub use loader::{BackendInitializationError, GgufBackendRuntime, GgufLoader};
pub use metadata::{GgufMetadata, MetadataError, inspect_metadata};
pub use model::{GgufModel, GgufSequence};
pub use source::{GgufExecutionConfiguration, GgufInspectionLimits, GgufSource, SourceError};

//! CPU Llama adapter that quarantines Candle behind `domain-contracts`.
//!
//! This adapter is the Phase 3 correctness backend. Candle's upstream Llama
//! implementation grows its KV cache with tensor concatenation, so this crate
//! deliberately does not advertise `CapabilitySet::ALLOCATION_FREE_HOT_PATH`.

#![forbid(unsafe_code)]

mod failure;
mod loader;
mod model;
mod source;

pub use loader::CandleLlamaLoader;
pub use model::{CandleLlamaModel, CandleLlamaSequence};
pub use source::{CandleLlamaSource, CandleScalarType, SourceError};

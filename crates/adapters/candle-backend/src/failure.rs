//! Stable conversion from Candle failures into allocation-free domain errors.

use domain_contracts::{BackendFailure, BackendFailureKind, BackendId};

pub const CODE_CONFIG_READ: u32 = 1;
pub const CODE_CONFIG_DECODE: u32 = 2;
pub const CODE_WEIGHT_METADATA: u32 = 3;
pub const CODE_WEIGHT_LOAD: u32 = 4;
pub const CODE_DUPLICATE_TENSOR: u32 = 5;
pub const CODE_MODEL_LOAD: u32 = 6;
pub const CODE_MODEL_LOAD_PANIC: u32 = 7;
pub const CODE_CACHE_CREATE: u32 = 8;
pub const CODE_INPUT_TENSOR: u32 = 9;
pub const CODE_FORWARD: u32 = 10;
pub const CODE_LOGITS_LAYOUT: u32 = 11;
pub const CODE_LOGITS_STORAGE: u32 = 12;
pub const CODE_SYNCHRONIZE: u32 = 13;
pub const CODE_RESERVATION: u32 = 14;
pub const CODE_NUMERIC_OVERFLOW: u32 = 15;

pub const fn failure(backend: BackendId, kind: BackendFailureKind, code: u32) -> BackendFailure {
    BackendFailure::new(backend, kind, code)
}

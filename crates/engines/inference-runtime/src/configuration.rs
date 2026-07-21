//! Validated runtime and host-worker limits.

use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::Duration;

use domain_contracts::MemoryBudget;
use host_runtime::ChannelCapacity;

/// Hard model, request, and aggregate memory bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// Maximum concurrently loaded model instances.
    pub maximum_loaded_models: NonZeroU32,
    /// Maximum active request-owned sequences across all models.
    pub maximum_active_requests: NonZeroU32,
    /// Aggregate resident memory budget enforced by the registry.
    pub memory_budget: MemoryBudget,
}

impl RuntimeLimits {
    /// Creates runtime limits from already validated non-zero counts.
    #[must_use]
    pub const fn new(
        maximum_loaded_models: NonZeroU32,
        maximum_active_requests: NonZeroU32,
        memory_budget: MemoryBudget,
    ) -> Self {
        Self {
            maximum_loaded_models,
            maximum_active_requests,
            memory_budget,
        }
    }
}

/// Bounded host-worker transport and lifecycle polling configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostedRuntimeConfiguration {
    /// Maximum queued runtime commands.
    pub command_capacity: ChannelCapacity,
    /// Maximum queued runtime events.
    pub event_capacity: ChannelCapacity,
    poll_interval_milliseconds: NonZeroU64,
}

impl HostedRuntimeConfiguration {
    /// Creates a hosted configuration.
    #[must_use]
    pub const fn new(
        command_capacity: NonZeroUsize,
        event_capacity: NonZeroUsize,
        poll_interval_milliseconds: NonZeroU64,
    ) -> Self {
        Self {
            command_capacity: ChannelCapacity::new(command_capacity),
            event_capacity: ChannelCapacity::new(event_capacity),
            poll_interval_milliseconds,
        }
    }

    /// Returns the lifecycle polling and bounded-send interval.
    #[must_use]
    pub const fn poll_interval(self) -> Duration {
        Duration::from_millis(self.poll_interval_milliseconds.get())
    }
}

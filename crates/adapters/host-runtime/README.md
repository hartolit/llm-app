# host-runtime

User-space infrastructure adapter for the local LLM workspace.

It quarantines Flume, `std::thread`, `Instant`, and the short-lived synchronization
used by frame-pull output batching. Engine crates receive stable wrapper types
rather than importing Flume directly.

The output accumulator allocates its byte and record storage once during setup.
Inference producers use non-blocking `try_lock`, validate both capacities before
mutation, and never resize the buffers. A UI consumer pulls and clears one
borrowed batch on its native frame clock while retaining the allocations.

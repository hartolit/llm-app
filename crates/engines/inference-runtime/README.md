# inference-runtime

Single-owner model registry and lifecycle engine.

The crate is generic over one concrete `ModelLoader`. It owns loaded model
weights, request sequences, generation-safe handles, aggregate memory admission,
cancellation, bounded drain escalation, synchronization, and unload. The
synchronous `InferenceRuntime` API is the source of truth; the hosted worker adds
bounded commands and events through `host-runtime` without changing ownership.

See the [inference runtime guide](../../../docs/project/inference-runtime.md) for
lifecycle guarantees and current concurrency limitations.

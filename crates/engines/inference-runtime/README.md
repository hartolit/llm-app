# inference-runtime

Single-owner model registry and lifecycle engine.

The crate is generic over one concrete `ModelLoader`. It owns loaded model
weights, request sequences, generation-safe handles, aggregate memory admission,
cancellation, bounded drain escalation, synchronization, and unload. Model load
and request start are prepare/validate/commit transactions: abandoned native
models call `prepare_unload`, abandoned native sequences call `destroy_sequence`,
and registry/accounting state is published only after validation succeeds. The
synchronous `InferenceRuntime` API is the source of truth; the hosted worker adds
bounded commands and events through `host-runtime` without changing ownership.

See the [inference runtime guide](../../../docs/project/inference-runtime.md) for
lifecycle guarantees and current concurrency limitations.

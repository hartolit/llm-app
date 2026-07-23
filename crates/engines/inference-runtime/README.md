# inference-runtime

Single-owner model registry, cleanup quarantine, and backend-independent generation engine.

The crate is generic over one concrete `ModelLoader`. It owns loaded model weights,
request sequences, generation-safe handles, aggregate memory admission, cancellation,
bounded drain escalation, synchronization, and unload. Model and request admission
use prepare/validate/commit transactions. If explicit rollback fails, the runtime
quarantines the only model or sequence cleanup handle, retains its memory and sequence
accounting, and reports both the primary and cleanup failure classifications.

`RuntimeCommand::Generate` admits an already-tokenized direct-completion request.
All logits, sampling, repetition-history, generated-token, and scheduler state is
reserved before backend sequence publication. The hosted worker then alternates
control command polling, one fair generation quantum, cleanup/unload maintenance,
and nonblocking output publication. Sampling executes inside this crate through the
portable `sampling` feature; a frontend never drives individual token steps.

Generated token IDs and ordered terminal state use `host-runtime`'s preallocated
pull accumulator. Full output capacity yields the request without another backend
step. Pulling a borrowed batch clears logical contents while retaining allocations,
after which generation resumes from the exact pending token.

See the [inference runtime guide](../../../docs/project/inference-runtime.md) for the
request lifecycle, cancellation boundary, finish states, and cleanup policy.

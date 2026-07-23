# Model Lifecycle and Cancellation Guarantees

`domain-contracts::ModelLifecycle` provides a deterministic policy state machine:

```text
Active
  -> Draining(deadline)
  -> Cancelling(DrainTimeout)
  -> Unloading
  -> Absent
```

The drain deadline is mandatory and non-zero. Expiration always returns
`LifecycleAction::CancelActive` with `CancellationReason::DrainTimeout`.

## Safe reclamation boundary

The state machine cannot safely destroy model resources while backend code still
owns a mutable borrow of the loaded model or sequence. Rust threads cannot be
forcibly terminated without violating resource and lock invariants. Therefore,
engine-level deterministic reclamation must use at least one of these execution
contracts:

1. backend prefill and decode calls have documented bounded duration and observe
   cancellation at safe boundaries;
2. long prefill work is split into bounded chunks controlled by the runtime;
3. an untrusted or potentially hanging backend runs in a separate process whose
   termination delegates final memory reclamation to the operating system.

A cooperative in-process backend may delay physical reclamation until its current
bounded step returns. The runtime must never drop a model concurrently with an
active backend call.

## Terminal cleanup and degraded state

All generation terminal paths—EOS, token limit, stop match, cancellation, backend
failure, sampling failure, drain escalation, and shutdown—use the same explicit
sequence-destruction transition. A destruction failure moves the sequence out of
the normal active-request registry into runtime-owned quarantine. Its identity,
model sequence slot, and memory footprint remain accounted, and the affected model
rejects new requests until cleanup succeeds.

Maintenance retries at most one quarantined cleanup operation per worker quantum.
A retry failure remains observable through the original allocation-free
`CleanupFailureReport`; a successful retry removes ownership and accounting exactly
once. Model unload preparation follows the same rule: failure retains the model and
its bytes, and success is the only permission to release it.

Generation output orders `Terminal`, optional `CleanupPending`, and `Released`
records. Consequently, completion of token generation is not presented as proof
that backend resources have already been released.

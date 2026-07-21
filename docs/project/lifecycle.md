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

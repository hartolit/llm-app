# Candle CPU Reference Backend

## Scope

`crates/adapters/candle-backend` is the Phase 3 reference implementation of the
`domain-contracts` backend boundary. It supports unquantized Hugging Face Llama
configuration files and one or more Safetensors weight shards on the CPU.

The adapter owns all Candle types. No Candle tensor, device, model, cache, or
error type crosses into a feature crate.

## Lifecycle

The loader performs three cold-path operations:

1. inspect configuration and weight-file metadata;
2. validate CPU device zero and the host-memory budget;
3. reserve the largest shard as transient loading headroom;
4. load Safetensors into Candle and construct the Llama model.

The loaded model exclusively owns weights. Each sequence owns an independent
Candle cache, position, fixed token capacity, and token staging allocation.
Sequences do not retain or clone the loaded model.

The compatibility test executes:

```text
inspect
→ plan and load
→ create two independent sequences
→ prefill both
→ interleave decode
→ cancel one at a checked boundary
→ reject unsupported in-place sequence reset
→ expire a bounded drain window
→ synchronize and prepare unload
```

## Allocation capability

The adapter intentionally does not advertise
`CapabilitySet::ALLOCATION_FREE_HOT_PATH`.

The upstream Candle 0.11 Llama implementation concatenates KV-cache tensors as
tokens are appended and constructs intermediate tensors during forward passes.
Claiming strict allocation-free execution would therefore be false even though
the adapter itself pre-reserves token staging and writes logits into
caller-owned slices.

A later engine may use this capability as an admission requirement. A future
strict backend must use pre-allocated KV-cache and execution arenas before it
sets the bit.

## Sequence reset capability

The adapter intentionally does not advertise `CapabilitySet::SEQUENCE_RESET`.
Candle's upstream Llama cache does not expose a way to clear its private KV and
mask state in place. Replacing the cache would allocate and would violate the
`LoadedModel::reset_sequence` contract. The adapter therefore returns
`SequenceError::Unsupported`; callers must destroy and recreate the sequence at
a cold lifecycle boundary.

## Failure containment

Candle failures are translated into allocation-free `BackendFailure` values
with stable categories and numeric codes. Candle's upstream Llama loader uses an
internal panic for a malformed layer, so model construction is isolated behind
`catch_unwind` at the cold adapter boundary and converted into an invalid-model
load failure.

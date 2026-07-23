# Current Implementation Status

**Status date:** 2026-07-23

**Source baseline:** uploaded Phase 3 archive, including `Cargo.lock`; repository VCS metadata was not included

**Execution position:** Phase 3 completion patch implemented at source level; the canonical locked validation gate must pass before Phase 4 starts

**Canonical plan:** [LLM App Execution Plan](../execution/execution-plan.md)

This document is the canonical statement of what the delivered source tree claims. It deliberately separates implemented source from validation evidence.

## Supported devices and backends

| Backend | Device | Adapter/E0 boundary | `application-runtime` (E1) | Slint UI |
|---|---|---:|---:|---:|
| Candle 0.11 Llama/Safetensors | CPU | Yes | Yes, lifecycle composition | Yes, lifecycle only |
| GGUF via llama.cpp | CPU | Yes | No | No |
| Candle or GGUF | CUDA/Metal/other GPU | No supported product path | No | No |

The repository is CPU-only today. Real-model prompt-to-token generation remains Phase 4; Phase 3 ordinary generation coverage uses a deterministic fake backend.

## Phase 3 source implementation

The source tree now contains the integrated backend-independent generation kernel:

- worker-owned prompt prefill, in-E0 sampling, incremental decode, bounded round-robin scheduling, cancellation, EOS, token-limit, and token stop-sequence handling;
- pull-oriented preallocated token/state output with nonblocking backpressure and ordered terminal records;
- explicit `GenerationOutputCapacityPolicy` admission against the hosted accumulator;
- preflight of prompt batch length, total sequence length, exact full-vocabulary logits capacity, model lifecycle/degraded state, identities, backend sequence memory, and generation host workspace memory before native sequence publication;
- generation workspace accounting for logits, sampling indices, repetition epochs, prompt/history/generated token storage, EOS storage, and stop-pattern storage;
- workspace accounting retained until terminal output release, even when backend sequence cleanup completed earlier;
- one cleanup state machine for admission rollback, completion, cancellation, backend/sampling failure, unload maintenance, drain escalation, and shutdown;
- allocation-free primary-plus-cleanup failure classification;
- quarantined model and sequence ownership with truthful memory/sequence accounting;
- deterministic total-attempt cleanup policy, one retry opportunity per maintenance loop, explicit retry/exhaustion state, and no retry after success or exhaustion;
- failed normal unload and shutdown unload routed through the same model quarantine policy;
- degraded-model admission rejection while a sequence remains quarantined;
- terminal explicit-shutdown and endpoint-disconnection policies that preserve unresolved native ownership rather than invoking an unverified implicit drop;
- shutdown termination independent of frontend token-output draining, with retained generation workspace accounting released before worker exit;
- deterministic fake-backend counters for loads, unload attempts, sequence creation/destruction, successful destruction, prefill/decode calls, sampling opportunities, active native resources, and retained simulated memory;
- fault-injection coverage added for cancellation before prefill, scheduled drain timeout, exact admission failures, repeated cleanup failure/exhaustion, model cleanup, shutdown cleanup, healthy-model isolation, later cleanup success, and exact single release.

`Cargo.lock` is present in the delivered tree.

## Integration depth

| Capability | E0 inference runtime | E1 application runtime | Slint UI |
|---|---:|---:|---:|
| Model load, generation-safe handle, drain, cancellation, unload | Yes | Yes for Candle lifecycle | Yes for Candle lifecycle |
| Backend prefill and decode primitives | Yes | Not exposed as generation | No |
| Backend-independent generation scheduler | Implemented with deterministic fake backend | Not exposed | No |
| Sampling algorithm | Integrated inside E0 | Not exposed | No |
| Bounded streamed token output | Pull-oriented token/state batches | No | No |
| Direct-completion real-model loop | Phase 4 | No | No |
| Tokenization and decoded text streaming | Separate foundations only | Not integrated | No |
| General chat templates/history | No | No | No |

## Validation status for this delivered patch

The canonical validation commands were **not executed in the artifact-editing environment** because it contains no Rust toolchain (`cargo`, `rustc`, or `rustfmt`) and external network access was unavailable. Therefore this document does not claim a passing test count, warning-free Clippy, rustdoc success, formatting success, or a completed Phase 3 gate for this exact archive.

Run the following from the repository root with the pinned toolchain in `rust-toolchain.toml`:

```text
cargo metadata --locked --format-version 1 --no-deps
cargo run --locked --bin llm-app -- verify
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny --workspace --locked check advisories bans licenses sources
git diff --check
```

Also run the configured portability and Markdown-link checks described by the execution plan and CI workflow. Phase 4 must remain gated until these commands pass on the exact delivered tree.

## Known limitations

- Phase 3 starts from caller-supplied token IDs and emits token IDs; tokenizer ownership, incremental text decoding, E1 generation commands, and frontend pulls remain later work.
- Real Candle/GGUF prompt-to-token generation is not claimed by this phase.
- The deterministic cleanup policy uses a total-attempt limit, not wall-clock retry backoff. One non-exhausted cleanup is attempted per worker maintenance loop.
- Exhausted resources remain quarantined and accounted until process termination or explicit future policy intervention; they do not re-enter normal registries.
- On endpoint disconnection, unresolved native resources are intentionally retained rather than implicitly dropped after failed explicit cleanup.
- GPU execution, remote/browser transport, general chat, GGUF UI selection, and multi-model E1 state are unsupported.

## Historical implementation record

The recovered [implementation plan](implementation-plan.md) is retained as historical context and is not authoritative. The execution plan supersedes its old phase sequence and proposed repository shape.

# Implementation Status

## Workspace baseline

The repository is a layered Cargo workspace with a root Rust maintenance runner.
It is pinned to Rust 1.96.1 and contains no Python, shell, PowerShell, or batch
automation.

## Phase 1

`domain-contracts` is the F0 feature foundation. It is always `no_std` and has
no workspace-local or third-party dependencies.

Implemented contracts include:

- strongly typed shared identifiers;
- capacity and error vocabulary;
- model and sequence contracts;
- checked prefill and decode entry points;
- deterministic lifecycle transitions;
- bounded drain timeout escalation;
- pull-oriented output batch contracts;
- an explicit allocation-free backend capability.

## Phase 2

Implemented F1 feature crates:

- `tokenization`: statically dispatched sinks, fixed token/byte/text buffers,
  incremental UTF-8 validation, and a stateful streaming-decoder contract;
- `context-planner`: deterministic budget selection with pinned admission,
  explicit provenance, caller-owned scratch, and original-order plans;
- `sampling`: zero-allocation temperature, top-k, top-p, min-p, repetition
  penalty, deterministic random selection, and token stop matching;
- `task-graph`: graph validation, cycle detection, attempt accounting,
  ready-task discovery, terminal exhaustion, cancellation, and blocked-descendant
  propagation.

Each F1 crate depends only on `domain-contracts` and never on another F1 crate.

## Phase 3

`candle-backend` is implemented under the adapter quarantine as a CPU reference
backend for unquantized Hugging Face Llama Safetensors.

It provides configuration inspection, memory admission, multi-shard loading,
independent sequence caches, checked prefill/decode, CPU-logit copying,
synchronization, unload preparation, stable failures, and generated-fixture
compatibility tests.

The adapter does not advertise `ALLOCATION_FREE_HOT_PATH`, because Candle 0.11's
Llama cache grows dynamically. It also does not advertise `SEQUENCE_RESET`,
because the upstream cache cannot be cleared in place. See
`docs/candle-backend.md`.

## Phase 4

Implemented runtime infrastructure:

- `host-runtime`: bounded Flume channels, named host threads, monotonic clocks,
  timeout translation, non-blocking thread-completion inspection, and a
  preallocated frame-pull output accumulator;
- `inference-runtime`: exclusive model registry ownership, generation-safe
  handles, request and sequence indexes, aggregate memory admission, checked
  prefill/decode, cancellation, bounded draining, unload, shutdown, snapshots,
  and a bounded hosted worker.

Lifecycle deadlines are polled independently of event delivery, so event
backpressure cannot suppress drain-timeout escalation. Backend calls remain
cooperative safe boundaries. See `docs/inference-runtime.md`.

## Phase 5

Implemented desktop infrastructure and reusable application orchestration:

- `hf-tokenizer`: Hugging Face Tokenizers adapter with generic caller-owned
  encode sinks and request-local stateful streaming decode;
- `hf-hub-adapter`: blocking repository inspection, commit-pinned cache
  resolution for `config.json`, `tokenizer.json`, supported unquantized
  Safetensors layouts, complete shard validation, and declared scalar detection;
- `redb-storage`: ACID application settings and logical model catalogue using
  explicit versioned binary records;
- `application-runtime`: E1 frontend-neutral coordinator for Hub workers,
  tokenizer validation, persistence, model loading, bounded drain/unload,
  normalized initial and terminal unload events, and bounded worker shutdown
  (see `docs/application-runtime.md`);
- `desktop-slint`: thin Slint presentation adapter and minimal binary entry point.

The Slint event loop polls `application-runtime` every 16 milliseconds and
processes a finite number of structured events per frame. No worker pushes
token-frequency callbacks into Slint. Alternative native frontends can consume
the same application runtime without importing Candle, Hugging Face, redb, or
Flume directly.

Workspace-owned source denies unsafe code. Pure crates retain explicit
`#![forbid(unsafe_code)]`. Slint's generated module requires a local
`allow(unsafe_code)`, so the workspace lint is intentionally `deny` rather than
`forbid`; project-authored Slint source still denies unsafe code.

Chat generation remains deferred until the context, sampling, streaming decode,
and runtime scheduling loop is connected and tested.

The current `hf-hub` synchronous API does not expose an application-level global
request timeout through its builder. Hub work is therefore isolated on a cold
worker, and shutdown waits for a bounded interval before detaching an in-flight
resolver so window closure cannot block indefinitely. See
`docs/desktop-runtime.md`.

## Phase 6

Implemented a second inference adapter:

- `gguf-backend`: bounded GGUF v2/v3 metadata inspection, explicit lower-bound
  memory admission, explicit process-level llama.cpp initialization, local CPU model
  loading, a preallocated native batch, a shared bounded KV arena, independent
  logical sequence slots, checked prefill/decode, reset, destruction, and unload;
- `inference-runtime`: the hosted worker no longer requires loaded models or
  sequence values to implement `Send`; they are constructed, used, and destroyed
  exclusively on the owning worker thread. The loader and source still cross the
  thread boundary and therefore retain `Send` requirements.

The GGUF adapter deliberately does not advertise `ALLOCATION_FREE_HOT_PATH`.
The safe upstream wrapper and llama.cpp retain internal execution storage. It
does advertise sequence reset because complete native sequence-cache removal is
available without rebuilding the model context. The `self_cell` expansion is
confined to one private generated-code module; handwritten adapter code remains
under `deny(unsafe_code)`. See `docs/gguf-backend.md`.

The Phase 5 tokenizer tests now use an explicit local test error, preserving the
portable tokenization error taxonomy without requiring it to implement
`std::error::Error`. Slint-generated API documentation warnings are confined to
the generated module; handwritten desktop source remains covered by workspace
documentation lints. Hosted draining now emits the original unload ticket both
when the timeout force-cancels work and when the final request finishes naturally.

## Phase 7

Implemented typed corrective workflow orchestration without adding a third engine:

- `task-graph`: semantic artifact roles, allocation-free artifact provenance
  validation, direct producer/consumer dependency validation, and attempt tokens
  that reject stale asynchronous completion;
- `application-runtime`: the canonical draft → validate → normalize diagnostics →
  review → revise → validate graph, immutable artifact storage, identifier-only
  task requests and events, restricted declared-input resolution, model-policy
  forwarding, deterministic diagnostic normalization, bounded aggregate storage and
  output contracts, operational retries, and accepted/rejected terminal outcomes;
- coarse `ModelTaskExecutor` and `ValidationTaskExecutor` ports keep model generation,
  compiler execution, and vendor-specific behavior outside the graph state machine.

Artifact and worst-case event capacity are admitted before any workflow side effect.
Artifacts are committed before their producing task transitions to success, and all
completion paths require the active `TaskAttempt`. A validator rejection is a
successful task result carrying typed diagnostics, while only operational failures
consume retry attempts. Full specification, draft,
review, and revision payloads are each stored once and downstream tasks receive
only `ArtifactId` values. See `docs/project/orchestration.md`.

## Phase 8

The initial optimization and enforcement baseline is implemented:

- all workspace-local dependency paths are centralized in the root
  `[workspace.dependencies]` table and member manifests inherit them;
- the native architecture validator has executable policy tests for feature,
  adapter, engine, and application tiers;
- application crates are restricted to the E1 `application-runtime` boundary
  rather than directly composing E0, adapters, or features;
- instrumented test allocators fail the portable checked prefill/decode and
  production sampling tests when prepared execution allocates or reallocates;
- the production default sampling pipeline has a Criterion benchmark over a
  vocabulary-sized flat working set, reporting per-sample latency and processed
  logit throughput with a documented cache footprint and machine-local baseline.
  See `docs/project/performance.md`.

The allocation gate covers project-owned `domain-contracts` dispatch and
caller-owned buffers. It does not claim that Candle or llama.cpp execution is
allocation-free; neither backend advertises that capability. Backend-native
allocation and observed-memory measurement, benchmarks for backend execution and
other portable algorithms, backend cache-footprint analysis, and timing stress tests
remain later Phase 8 work.

## Native maintenance runner

```text
cargo fmt --all -- --check
cargo run --bin llm-app -- architecture
cargo run --bin llm-app -- benchmark
cargo run --bin llm-app -- check
cargo run --bin llm-app -- test
cargo run --bin llm-app -- fmt
cargo run --bin llm-app -- fmt-check
cargo run --bin llm-app -- clippy
cargo run --bin llm-app -- verify
```

## Verification state

The current Phase 8 baseline is validated with the complete native maintenance
sequence above. Desktop launch remains a separate target-machine integration
check because Slint opens a native window and the GGUF adapter compiles upstream C/C++ code:

```text
cargo run -p desktop-slint
```

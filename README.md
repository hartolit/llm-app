# HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced

# llm-app

A layered Rust workspace for local language-model inference, context planning,
sampling, orchestration, persistence, and replaceable application frontends.

The repository currently contains:

- Phase 1: portable backend and lifecycle contracts;
- Phase 2: tokenization, context planning, sampling, and task-graph features;
- Phase 3: Candle CPU Llama reference backend;
- Phase 4: exclusive-ownership inference runtime and bounded host transport;
- Phase 5: Hugging Face artifact/tokenizer adapters, redb persistence, a
  frontend-neutral application runtime, and a thin Slint runner;
- Phase 6: a second CPU backend for local GGUF models through llama.cpp.

The Phase 5 application runtime resolves a mutable Hub reference to one immutable
commit, caches that exact artifact set, validates the tokenizer and declared
scalar type, persists logical model selections, and loads or unloads one CPU
model generation. Slint only maps callbacks and structured events to widgets.
A Tauri, CLI, or another native frontend can consume `application-runtime`
without duplicating Hub, storage, tokenizer, or inference lifecycle logic.

Chat generation remains deliberately absent until context planning, prompt
rendering, sampling, streaming decode, and generation scheduling are connected
as one tested loop.

## Workspace

```text
crates/features/domain-contracts
crates/features/tokenization
crates/features/context-planner
crates/features/sampling
crates/features/task-graph
crates/adapters/candle-backend
crates/adapters/gguf-backend
crates/adapters/host-runtime
crates/adapters/hf-tokenizer
crates/adapters/hf-hub
crates/adapters/redb-storage
crates/engines/inference-runtime
crates/engines/application-runtime
crates/apps/desktop-slint
```

`domain-contracts` is the shared F0 leaf foundation. F1 feature crates may
depend on it but never on one another. `inference-runtime` is the E0 model and
sequence owner. `application-runtime` is the E1 use-case coordinator and may
depend downward on E0. Vendor, filesystem, network, database, and operating-
system dependencies remain quarantined in adapters.

## Validate

Use the explicit root Rust runner binary:

```text
cargo run --bin llm-app -- verify
```

Individual commands are available through:

```text
cargo run --bin llm-app -- help
```

The root package declares `default-run = "llm-app"`, but workspace binary
selection can still vary with Cargo invocation context. Documentation therefore
uses the explicit binary form so commands remain unambiguous as more runners are
added.

Plain Cargo commands work normally:

```text
cargo check
cargo test
```

## Slint frontend

```text
cargo run -p desktop-slint
```

The initial frontend still uses the CPU Candle backend; Phase 6 adds the GGUF
adapter at the engine boundary but does not yet expose backend selection in the
UI. Application state is stored in the platform's per-user application-data
directory. See `docs/application-runtime.md`, `docs/desktop-runtime.md`, and
`docs/gguf-backend.md` for the relevant boundaries.

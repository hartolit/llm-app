# Workspace Boundaries

## Physical layout

```text
llm-app/
├── Cargo.toml
├── src/main.rs
├── docs/
└── crates/
    ├── features/
    │   ├── domain-contracts/
    │   ├── tokenization/
    │   ├── context-planner/
    │   ├── sampling/
    │   └── task-graph/
    ├── adapters/
    │   ├── candle-backend/
    │   ├── gguf-backend/
    │   ├── host-runtime/
    │   ├── hf-tokenizer/
    │   ├── hf-hub/
    │   └── redb-storage/
    ├── engines/
    │   ├── inference-runtime/
    │   └── application-runtime/
    └── apps/
        └── desktop-slint/
```

The root package is a native Rust maintenance runner. Product execution vectors
remain under `crates/apps/`.

## Feature dependency tiers

```text
F1 algorithmic features
├── tokenization
├── context-planner
├── sampling
└── task-graph
          ↓
F0 shared contract foundation
└── domain-contracts
```

`domain-contracts` is the sole F0 leaf. It owns vocabulary that genuinely
crosses engine/backend or multiple-feature boundaries: strongly typed IDs,
capacity failures, model and sequence contracts, lifecycle transitions, and
output records. It has no workspace-local dependencies.

F1 crates may depend downward on `domain-contracts`. They may not depend on one
another. This avoids duplicated IDs without splitting identifiers, capacities,
and metadata into micro-crates.

## Engine dependency tiers

The two engine crates are cohesive rather than interchangeable:

```text
E1 application use-case orchestration
└── application-runtime
          ↓
E0 inference resource ownership
└── inference-runtime
```

`inference-runtime` owns loaded model generations, request sequences, admission,
cancellation, draining, and unload. It does not know about Hub repositories,
persistence, tokenizers, or UI state.

`application-runtime` coordinates Hub resolution, tokenizer validation,
persistence, and inference lifecycle for host frontends. It may depend downward
on `inference-runtime`; the reverse edge is forbidden. This explicit E1 → E0
edge permits Slint, Tauri, CLI, or another frontend to reuse one application
workflow without placing vendor concerns inside the inference owner.

Adding an engine crate requires architectural review and evidence of independent
ownership, lifecycle, or reuse. The project has no numerical crate quota.

## Layer direction

```text
apps
  ↓
E1 application-runtime
  ↓
E0 inference-runtime
  ↓
adapters and features
  ↓
domain-contracts
```

The diagram expresses permitted composition, not a requirement that every crate
traverse every layer. Adapters quarantine vendor, FFI, filesystem, network,
database, and OS dependencies. Engines coordinate state and lifetimes.
Applications own the event loop, environment-specific paths, and presentation.

No production crate may depend upward. Adapter crates do not import one another. Development dependencies are reviewed separately and may cross production direction only for an explicitly named compatibility test or benchmark.

## Current members

```text
.
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

Each feature and engine crate owns a complete, independently testable domain.
None exists merely to hold one identifier, one data structure, or one callback.

## Production dependency edges

```text
candle-backend      -> domain-contracts
gguf-backend        -> domain-contracts
host-runtime        -> domain-contracts
hf-tokenizer        -> tokenization -> domain-contracts
inference-runtime   -> host-runtime + domain-contracts
application-runtime -> inference-runtime + selected adapters/features
desktop-slint       -> application-runtime + slint
```

`desktop-slint` no longer imports Candle, Hugging Face, redb, host channels, or
inference commands directly. Slint types remain confined to the application
crate, and application-runtime public events expose stable application/domain
values rather than vendor types.

The validator uses typed Cargo metadata, fails closed on unknown workspace
locations and path targets, distinguishes dependency kinds, and applies the
external dependency rules documented in the [dependency policy](dependency-policy.md).

## Generated-code lint boundary

Workspace-owned source denies unsafe code. Most pure crates additionally use
`#![forbid(unsafe_code)]`. The workspace-level lint is `deny`, not `forbid`,
because Slint and `self_cell` generate Rust that applies a narrow local
`allow(unsafe_code)` inside private generated-code modules. `forbid` cannot be
lowered by generated code and therefore makes those valid expansions
uncompilable. These boundaries do not permit unsafe blocks in project-authored
source.

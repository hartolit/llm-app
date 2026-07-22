# Architecture

This document is the current normative architecture policy. The [documentation map](README.md) defines its authority relative to ADRs, status, component guides, historical plans, and knowledge notes.

## Rule classes

Every architectural statement should be understood as one of these classes:

- **Hard invariant** — required for correctness, safety, or a deliberately enforced boundary. A change requires implementation evidence and, when architectural, an ADR.
- **Current decision** — the selected design for the present product. It may change through an ADR when a real use case provides contrary evidence.
- **Performance hypothesis** — a claim that must be supported by a named benchmark, allocation test, profile, or generated-code inspection before it becomes a requirement.
- **Style preference** — a default that improves consistency but may yield to clearer code.
- **Temporary constraint** — a deliberate limit while the first product slice is being proved.

## Hard invariants

### Ownership and lifecycle

- The inference runtime exclusively owns loaded model and sequence values during normal execution. Public handles carry identity, not shared model ownership.
- Model, sequence, request, cancellation, drain, unload, and shutdown transitions are explicit and bounded where the underlying backend permits bounded progress.
- Native resources are not destroyed while backend code holds mutable access to them.
- Commands, event queues, workspaces, and output accumulation use explicit capacities where unbounded growth could violate runtime behavior.

### Dependency direction

- The workspace dependency graph is acyclic.
- Portable feature contracts do not import engines, applications, vendor runtimes, filesystem/network/database clients, frontend toolkits, or OS transport implementations.
- Adapters do not import engines or applications.
- `inference-runtime` does not import `application-runtime` or an application.
- Application production code enters model lifecycle behavior through `application-runtime`, rather than bypassing it to compose E0 and adapters independently.

The current validator enforces workspace-local path dependencies only. It does not yet fail closed on unknown paths, distinguish dependency kinds, or enforce external dependency policy; those are Phase 1 work and must not be claimed as current guarantees.

### Unsafe code

Project-authored Rust denies unsafe code. Generated-code or FFI containment exceptions must be narrow, documented, and kept inside the adapter or generated module that requires them. Safe types must prevent native pointers or invalid lifetimes from escaping those boundaries.

## Current decisions

### Physical layout

The repository uses these categories:

```text
crates/features/    portable contracts and algorithms
crates/adapters/    infrastructure and vendor implementations
crates/engines/     stateful orchestration and resource ownership
crates/apps/        process, event-loop, and presentation boundaries
```

Folder names communicate ownership but have no Cargo semantics. Crates move only when ownership, reuse, or dependency evidence justifies the change; path churn is not an architecture improvement by itself. See [ADR-0005](decisions/0005-retain-crate-folders.md).

### Feature tiers

`domain-contracts` is the current F0 shared foundation. `tokenization`, `context-planner`, `sampling`, and `task-graph` are F1 algorithm crates.

The currently enforced production policy allows F1 → F0 and rejects F1 → F1. This is a **temporary constraint**, not a universal Rust principle. It remains in force until the generation slice supplies enough evidence to replace it with an approved acyclic dependency graph. New shared vocabulary must not be pushed into `domain-contracts` merely to evade the policy.

### Engine tiers

- E0 `inference-runtime` owns models, sequences, requests, admission, cancellation, draining, and unload.
- E1 `application-runtime` is the frontend-neutral application façade and current native composition root. It owns artifact resolution, tokenizer validation, persistence, normalized application state/events, and model lifecycle use cases.
- E1 may depend on E0. E0 may not depend on E1.

Keeping E1 as the reusable frontend boundary is [ADR-0001](decisions/0001-application-runtime-facade.md). It should not be made generic over every service. Cold, coarse replacement points may use trait objects or closed enums; token-sensitive model execution remains statically dispatched.

### Frontends and deployment

Slint, a native Tauri host, a CLI, or another native process may call `application-runtime` directly. A browser-only frontend cannot own Candle, llama.cpp, redb, native threads, and filesystem paths; it requires an explicit transport to a native or remote host.

The frontend presents state and pulls bounded output. It does not drive one inference command per generated token. Generation scheduling belongs beside model execution as recorded by [ADR-0003](decisions/0003-generation-scheduling-ownership.md).

## Temporary product constraints

Until the first streamed generation slice is complete:

- CPU is the only supported device class;
- Candle is the composed E1/UI backend;
- GGUF exists only at the adapter/E0 compatibility boundary;
- E1 represents one resident model generation;
- direct completion is the first planned generation mode;
- general chat templates, remote transport, GPU execution, and broad crate reorganization are deferred.

The backend and prompt sequencing decisions are recorded in [ADR-0002](decisions/0002-candle-cpu-first-vertical-slice.md) and [ADR-0004](decisions/0004-direct-completion-before-chat.md).

## Performance hypotheses and evidence

- Static dispatch is required in measured token/tensor loops where it materially affects code generation. Dynamic dispatch is permitted for cold, coarse operations such as storage, artifact resolution, configuration, and backend selection.
- Allocation-free behavior may be required for a named project-owned hot path only when a test defines the measured region. It is not inferred from `no_std`, caller-owned output, or adapter boundaries.
- `#[inline]`, `#[inline(always)]`, `#[cold]`, layout changes, smaller integer types, and custom data layouts require profiling or generated-code evidence when used as optimizations.
- Component benchmarks establish component behavior only. They do not prove end-to-end latency or throughput.

## Crate and module design preferences

Crates should follow cohesive ownership, independent reuse, dependency direction, and meaningful test boundaries. There is no numerical crate quota. Before extracting a crate, prefer an internal module split when the code shares one lifecycle and has no independent consumer.

Public APIs should expose the minimum stable vocabulary required by callers. Internal helpers use the narrowest practical visibility. Dependency injection is preferred over hidden global state, but type parameters should not spread through public application APIs without a hot-path or reuse reason.

## Shutdown

Callers must use explicit bounded shutdown for workers and model resources. Blocking `Drop` is not the primary shutdown mechanism; a best-effort drop path cannot replace an observable shutdown result. See [ADR-0006](decisions/0006-explicit-bounded-shutdown.md).

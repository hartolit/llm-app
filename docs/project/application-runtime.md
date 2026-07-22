# Application Runtime

## Responsibility

`application-runtime` is the E1 frontend-neutral use-case engine. It coordinates
cold-path infrastructure without absorbing tensor execution or UI behavior.

It owns:

- persisted application preferences;
- one bounded synchronous Hub worker;
- immutable artifact resolution;
- tokenizer validation and vocabulary compatibility;
- exact repository/revision selection checks;
- one hosted `inference-runtime` endpoint;
- model load, bounded drain, terminal unload completion, and shutdown commands;
- typed corrective workflow execution over `task-graph`;
- immutable in-process workflow artifacts and identifier-only routing;
- deterministic diagnostic normalization, retries, and terminal validation outcomes;
- normalized application state and events.

It does not own:

- Slint, Tauri, Leptos, terminal, or HTTP types;
- model tensors or per-sequence inference state;
- prompt rendering, sampling, or generation scheduling;
- OS-specific application-data path policy.

## Public boundary

Frontends construct `ApplicationRuntimeConfiguration`, start
`ApplicationRuntime`, inspect `ApplicationState`, submit coarse model-lifecycle
use cases, and poll `ApplicationEvent` values.

Corrective workflows use the separately composable
`CorrectiveWorkflowExecutor<M, V>`. Concrete model and validator services implement
`ModelTaskExecutor` and `ValidationTaskExecutor`; the E1 executor owns graph state,
retry accounting, immutable artifacts, diagnostic normalization, and identifier-only
workflow events. This boundary does not move prompt rendering, sampling, tensor
execution, or compiler process policy into E1.

The public boundary exposes application-owned values and `domain-contracts`
types. Candle, Hugging Face, redb, Flume, and inference command/event types remain
private implementation details.

Immediate admission or queue failures are returned as `ApplicationError`.
Asynchronous worker outcomes are returned as structured `ApplicationEvent`
values. Vendor failures are normalized into `ApplicationFailure` with a stable
category and owned cold-path diagnostic.

## Engine tiers

```text
frontend
   ↓
application-runtime (E1)
   ↓
inference-runtime (E0)
   ↓
adapters and feature contracts
```

E1 may depend on E0. E0 never imports E1. This keeps exact model-resource
ownership independent from repository, persistence, and presentation workflows.

## Frontend replacement

A native Tauri backend or CLI runner can depend directly on
`application-runtime` and map its events to another presentation layer. A
standalone browser frontend cannot run Candle or redb directly; it should use a
transport adapter to a native or remote host that owns `ApplicationRuntime`.

No frontend should reimplement Hub resolution, tokenizer validation, model
compatibility checks, unload timing, corrective graph transitions, artifact
provenance, or retry accounting. See `docs/project/orchestration.md` for the Phase 7
workflow boundary.

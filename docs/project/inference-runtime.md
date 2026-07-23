# Inference Runtime

`crates/engines/inference-runtime` is the Phase 4 single-owner model registry.
It is generic over one concrete `ModelLoader` and owns every loaded model,
request sequence, lifecycle state, and aggregate memory reservation.

## Ownership

```text
InferenceRuntime<L>
├── loader: L
├── model registry
│   └── ModelSlot<L::Model>
│       ├── exclusively owned model weights
│       ├── ModelLifecycle
│       └── request-owned backend sequences
├── request index
├── sequence index
└── aggregate memory accounting
```

Public handles contain only typed identifiers and model generations. Models and
sequences are never stored in `Arc`, cloned into clients, or borrowed across the
command boundary.

## Admission

A load is admitted only when all of the following hold:

- the logical model identity is not resident;
- the configured loaded-model limit has capacity;
- the backend load plan fits the remaining aggregate host and device budgets;
- the returned model handle and metadata match the accepted plan.

A request sequence is admitted only when:

- its request and sequence identifiers are unused;
- the runtime-wide active-request limit has capacity;
- the backend model's maximum sequence count has capacity;
- its sequence plan fits the remaining aggregate memory budget.

Admission uses prepare/validate/commit transactions. A newly loaded model remains
local until its handle, metadata, and lifecycle have been validated; any
post-load failure calls `prepare_unload` before the model is dropped. A newly
created sequence remains local until its identity, token capacity, lifecycle,
and every registry entry are ready to commit; any abandoned sequence calls
`destroy_sequence`. Backend plans that contradict the requested sequence
configuration are rejected before native creation.

Accounting and indexes are updated only in the final commit after successful
ownership transfer. A failed transaction therefore leaves no model/request
registry or memory-accounting mutation. Sequence bytes are released when the
request is completed or cancelled. Model bytes are released only after
`prepare_unload` and the lifecycle's `complete_unload` transition succeed.

## Drain and cancellation

`UnloadPolicy::Drain` enters the domain lifecycle's mandatory bounded drain
window. The hosted worker polls pending unload transitions on its monotonic clock before
command receipt and before every bounded event-send retry. Therefore an
event consumer that stops draining its bounded queue cannot prevent the drain
deadline from escalating to cancellation. The worker retains the originating
unload ticket and publishes a second terminal `ModelUnload` event after timeout-driven
cancellation and resource release, so frontends cannot remain stuck in a draining
state.

The worker is single-owner and invokes backends synchronously. It can force
cancellation as soon as the current backend call returns to a safe boundary. It
cannot safely terminate a Rust thread while a backend call holds mutable model
state. Backends with potentially unbounded calls still require bounded kernels,
chunked prefill, or process isolation.

## Hosted transport

`crates/adapters/host-runtime` quarantines:

- Flume bounded MPMC channels;
- named host threads;
- monotonic `Instant` measurement;
- timeout and disconnection translation.

The engine exposes owned commands and events. Prefill and decode commands move a
caller-provided `Vec<f32>` into the worker and return the same allocation in the
event, allowing callers to recycle storage without per-step vector allocation.
This command/event path is a control and compatibility interface, not the final
high-throughput generation loop. Sampling and output decoding will move inside
the worker in a later phase so one token does not require a UI or application
round trip.

## Current backend composition

One `InferenceRuntime<L>` instance is monomorphized for one loader/model family.
This avoids a dynamic-dispatch axis inside prefill and decode. An application can
run multiple runtime instances or later provide a coarse backend enum without
changing the model and sequence contracts.

## Sequence cleanup and terminal unload events

Backends release sequence-owned or model-owned native state through the borrowed
`destroy_sequence` hook. A cleanup failure leaves the request, sequence identity,
and memory accounting intact so a later lifecycle poll can retry the same native
release safely.

The hosted worker retains the original unload ticket while a model drains. It emits
a terminal `Unloaded` event with that ticket after either timeout escalation or the
request that naturally reduced the active count to zero. Event-queue backpressure
delays delivery but does not suppress cleanup or the terminal receipt.

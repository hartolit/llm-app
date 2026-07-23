# Inference Runtime

`crates/engines/inference-runtime` is the E0 single-owner model registry and
backend-independent generation scheduler. It is generic over one concrete
`ModelLoader` and owns every loaded model, backend sequence, generation workspace,
lifecycle transition, and aggregate memory reservation.

## Ownership and accounting

```text
Hosted worker
├── InferenceRuntime<L>
│   ├── normal model registry
│   │   └── ModelSlot<L::Model>
│   │       ├── exclusively owned model
│   │       ├── ModelLifecycle
│   │       ├── active request sequences
│   │       └── quarantined sequences
│   ├── quarantined post-load models
│   ├── active and pending-cleanup identity indexes
│   └── aggregate normal + quarantined memory accounting
├── fair generation scheduler
└── nonblocking token-output producer
```

Models and sequences are never placed in `Arc` or borrowed across the command
boundary. Public clients retain only typed identifiers and generation-safe model
handles. A resource remains counted until its explicit backend cleanup succeeds.
`RuntimeSnapshot` distinguishes active requests, pending model cleanup, pending
sequence cleanup, and total reserved memory. Per-model snapshots expose degraded
state and pending sequence counts.

## Transaction and cleanup semantics

Model and sequence admission follow prepare, validate, commit. Host-side generation
workspaces are reserved before sequence creation. Registry indexes, lifecycle state,
and normal active-request accounting are published only after validation succeeds.

Cleanup failure does not imply release:

- a model that fails post-load validation and `prepare_unload` is retained outside
  the normal model registry;
- an uncommitted sequence whose `destroy_sequence` fails is retained outside the
  active request registry;
- a terminal request whose sequence destruction fails moves from active ownership
  to quarantine;
- quarantined bytes and sequence slots remain admitted against hard limits;
- an affected model is degraded and rejects new requests;
- `poll_cleanup` attempts at most one retained operation per call;
- successful retry releases identity, capacity, and memory exactly once.

`CleanupFailureReport` is allocation-free and preserves the primary operation and
failure class independently from the cleanup operation and failure class. It avoids
recursive boxed error chains while retaining stable categories for later E1
translation.

Backend cleanup hooks are retry contracts: `destroy_sequence(&mut sequence)` must
leave the borrowed sequence valid after failure, and `prepare_unload(&mut model)`
must leave the model valid after failure. The runtime never treats unverified
`Drop` behavior as successful explicit cleanup.

## Generation admission

`RuntimeCommand::Generate` carries the minimum token-level runtime request:

- request and sequence identity;
- prompt token storage;
- sequence capacity and maximum generated tokens;
- sampling configuration and seed;
- EOS tokens and owned token stop patterns;
- scheduler quantum.

It does not carry tokenizer objects, decoded text, paths, display strings, frontend
DTOs, or UI state. Before backend sequence creation, E0 validates prompt and total
sequence length, model state, identities, and sampling configuration, then reserves:

- vocabulary-sized logits;
- sampling indices and repetition epochs;
- prompt/repetition history;
- generated-token history;
- terminal and backpressure state.

The backend still prepares its sequence-owned prefill/decode workspace through its
normal `SequencePlan`. No vector resize occurs in the scheduler decode loop.

## Scheduler lifecycle and fairness

A scheduled request moves through explicit phases:

```text
admitted -> prefill -> pending token publication -> decode
    -> terminal publication -> cleanup pending (optional) -> released
```

The worker checks one control command, advances one request by a bounded opportunity,
processes one cleanup retry and unload maintenance, and flushes bounded events on
each loop. Request selection uses a rotating ordered cursor, so runnable requests
each receive an opportunity. A request waiting on full output does not perform
another backend step and therefore cannot monopolize model execution.

The current scheduler intentionally performs at most one token-producing backend
step before token publication even if a larger configured quantum is retained. This
is the correctness baseline; later measured tuning may batch a small number of
steps without changing the contract.

Prefill occurs once. Sampling runs inside E0 immediately from checked logits using
request-owned `sampling::Sampler` state. The selected token is appended to bounded
history before any subsequent decode. EOS, generated-token limit, and token stop
suffixes are checked after ordered token publication.

## Pull output and backpressure

`host-runtime` supplies a separate token accumulator rather than encoding token IDs
as UTF-8 byte ranges. It preallocates token and record vectors during worker setup.
The producer uses `try_lock`; the application pulls a borrowed batch and clears its
logical contents while retaining both allocations.

Records preserve request identity and contain either an absolute monotonic token
range or one `GenerationOutputState`:

- `Yielded(OutputBackpressure)`;
- `Terminal(original outcome)`;
- `CleanupPending { original outcome, failure report }`;
- `Released(original outcome)`.

When token or record capacity is full, the sampled token remains request-owned,
no decode step is performed, and no token is discarded or emitted twice. After a
pull frees capacity, the yield record and exact pending token are published before
decode resumes. Generation completion and backend resource release are therefore
observable as separate ordered facts.

## Cancellation, unload, and shutdown

User cancellation is recorded as a control operation and observed before the next
backend step. Latency is bounded by one currently executing backend operation, the
one-step correctness quantum, and the worker command polling cadence. Cancellation
always enters the same terminal cleanup path as EOS, token limits, stop patterns,
and failures.

Immediate model unload marks scheduled requests with `ModelUnload`; drain timeout
maintenance marks them with `DrainTimeout`. The runtime may have already destroyed
the sequence at that safe boundary, but the scheduler still publishes the stable
cancellation outcome. Runtime shutdown marks scheduled work with `RuntimeShutdown`
and performs explicit sequence/model cleanup; the worker remains alive long enough
to publish retained terminal output when downstream capacity is available.

## Scope after Phase 3

Phase 3 is backend-independent and uses deterministic fake models in ordinary CI.
It does **not** claim real Candle/GGUF generation, tokenizer integration, decoded
text streaming, E1 generation commands, UI generation, chat templates, or GPU
execution. Those remain later execution-plan phases.

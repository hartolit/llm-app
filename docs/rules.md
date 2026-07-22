# General Engineering Rules

The [documentation map](README.md) defines the authority of this document. Rules are classified so safety requirements are not confused with preferences or unmeasured performance claims.

## Hard invariants

### Production behavior

- Merged production paths must be complete, compile-ready, and honest about unsupported behavior.
- Do not use placeholder results, fabricated backend behavior, silent truncation, or `TODO` branches as if they were product functionality.
- Tests may use deterministic fakes, stubs, fixtures, and fault-injection implementations. They are required when real dependencies cannot reliably reproduce rollback, timeout, or contract-violation paths.
- Every new invariant and reproduced failure needs a focused test when the repository can exercise it deterministically.

### Error and resource handling

- Return typed `Result`/`Option` outcomes for recoverable failures. Do not use panic-based control flow.
- `.unwrap()` and `.expect()` remain denied by workspace lint policy. A proven invariant should still normally use an explicit branch or typed conversion that makes failure semantics reviewable.
- Validate capacities and arithmetic before mutating externally visible state. Multi-step resource operations must either commit completely or roll back explicit native resources and accounting.
- Cancellation, drain, unload, and shutdown behavior must identify bounded safe points and the behavior of an uncooperative dependency.
- Secrets, tokens, and credentials must not be hardcoded or committed.

### Unsafe and native boundaries

- Project-authored source denies unsafe code unless an explicitly reviewed boundary requires it.
- Every authored unsafe operation must state the safety preconditions and why they hold. Generated code and third-party macros are confined to the narrowest module that needs their lint exception.
- Raw native pointers, invalid borrowing relationships, and vendor error types must not escape a safe adapter boundary.

## Current decisions

- The workspace is pinned by `rust-toolchain.toml`; the current stable compiler is Rust 1.96.1.
- Follow edition 2024 idioms supported by the pinned toolchain. “Modern” does not mean adopting unstable or unnecessary features.
- Preserve public APIs unless the active work package explicitly authorizes a change.
- Use the current repository verification command documented in the [execution plan](execution/execution-plan.md).
- Update the canonical [implementation status](project/implementation-status.md) when behavior, support, validation evidence, or the active phase changes.
- Record architectural changes in an ADR instead of silently rewriting policy.

## Performance hypotheses

- Optimize a named hot path only after a benchmark, allocation gate, profile, or generated-code inspection identifies the cost.
- Static dispatch and preallocated buffers are defaults for token/tensor loops. They are not blanket requirements for cold service boundaries.
- Do not claim allocation-free, portable, backend-neutral, chat-compatible, or GPU-capable behavior without a named test or measurement defining the scope.
- Compiler attributes and data-layout changes are hints or tradeoffs, not guarantees. Measure before and after on the same toolchain and representative workload.
- Shared-CI wall-clock timing is observational unless the environment is controlled; deterministic correctness and allocation tests may be hard gates.

## Style preferences

- Names should communicate domain meaning. Familiar short terms are acceptable when they are standard in the local domain; avoid unexplained abbreviations.
- Comments explain non-obvious intent, invariants, safety, or tradeoffs. Do not narrate obvious syntax or use source comments as a changelog.
- Prefer cohesive modules and crates over both god modules and one-type micro-crates. Crate count follows ownership and reuse, not a quota.
- Prefer centralized typed configuration for policy values. A local constant is appropriate when a value is genuinely local and named; not every numeric literal is a configuration option.
- Favor readable idiomatic Rust on cold paths. Introduce complex type-state, custom collections, or service abstractions only when they prevent a demonstrated class of errors or support a real consumer.

## Experimental work

Spikes and experimental branches are allowed to answer uncertain design or performance questions. They must be clearly identified and need not satisfy production completeness while isolated. Before merge, either convert the result into tested production behavior or discard it; experimental shortcuts must not be presented as supported functionality.

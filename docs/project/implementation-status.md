# Current Implementation Status

**Status date:** 2026-07-22

**Source baseline:** commit `51c543d6ad4ab3fc5acfe0da2d4c2fe4e3c8a168`

**Execution position:** Phase 0 documentation baseline complete; Phase 1 quality-gate work is next

**Canonical plan:** [LLM App Execution Plan](../execution/execution-plan.md)

This is the canonical statement of what the repository supports now. Historical phase numbering in older component guides describes when code was introduced; it is not the active roadmap.

## Supported devices and backends

| Backend | Device | Adapter/E0 boundary | `application-runtime` (E1) | Slint UI |
|---|---|---:|---:|---:|
| Candle 0.11 Llama/Safetensors | CPU | Yes | Yes, currently composed | Yes, lifecycle only |
| GGUF via llama.cpp | CPU | Yes | No | No |
| Candle or GGUF | CUDA/Metal/other GPU | No supported product path | No | No |

The repository is CPU-only today.

The Candle adapter targets unquantized Hugging Face Llama configuration and Safetensors weights. The application runtime resolves Hugging Face artifacts, validates the Hugging Face tokenizer and scalar declaration, persists one logical selection, and loads one Candle model generation on CPU.

The GGUF adapter supports the backend contracts and compile/test compatibility at the lower inference boundary. It does not yet have the tokenizer, E1 composition, or UI backend selection required for a product path.

## Integration depth

| Capability | E0 inference runtime | E1 application runtime | Slint UI |
|---|---:|---:|---:|
| Model load, generation-safe handle, drain, cancellation, unload | Yes | Yes for Candle | Yes for Candle |
| Hugging Face resolve/tokenizer validation/persistence | N/A | Yes | Yes |
| Backend prefill and decode primitives | Yes | Not exposed as generation | No |
| Sampling algorithm | Separate feature crate | Not integrated | No |
| Context planning | Separate feature crate | Not integrated | No |
| Direct-completion prompt-to-stream loop | No | No | No |
| General chat templates/history | No | No | No |
| Bounded streamed text output | Transport primitives only | No | No |
| Corrective workflow graph | N/A | Yes, separately composable | No product surface |

No current user-facing path performs prompt → tokenize → context admission → prefill → sample → incremental decode → bounded text streaming. Direct completion is the first planned generation mode. General chat support follows only after that loop is proven.

## Implemented foundations

- Portable `domain-contracts`, tokenization, context-planning, sampling, and task-graph crates.
- Candle CPU and GGUF CPU adapters with backend contract tests.
- Exclusive-owner inference runtime with bounded hosted transport, lifecycle state, memory admission, cancellation, draining, and unload.
- Frontend-neutral application façade for Hugging Face resolution, tokenizer validation, redb persistence, Candle model lifecycle, normalized events, and corrective workflows.
- Thin Slint lifecycle frontend.
- Package-local correctness, allocation, compatibility, and workflow tests plus one Criterion sampling benchmark.

Component details are indexed by the [documentation map](../README.md).

## Reproducible validation evidence

Baseline command run on 2026-07-22 from source commit `51c543d6ad4ab3fc5acfe0da2d4c2fe4e3c8a168`:

```text
cargo run --bin llm-app -- verify
```

Toolchain:

```text
rustc 1.96.1 (31fca3adb 2026-06-26)
cargo 1.96.1 (356927216 2026-06-26)
```

Result: passed. At this commit, `verify` ran:

- the workspace-local path architecture validator;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --all-targets --all-features`;
- `cargo test --workspace --all-targets --all-features`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

This is local command evidence, not a CI run. Because the current test wrapper uses `--all-targets`, it also selects the Criterion benchmark target; Phase 1 will separate ordinary tests from benchmark compilation.

The Phase 0 documentation changes were also checked for repository-local Markdown targets and stale legacy `docs/*.md` references. No product binary or real external model was exercised by documentation validation.

## Known limitations

- There is no CI workflow or required remote quality gate yet.
- Architecture validation only inspects workspace-local path dependencies, treats unknown locations as applications, and does not distinguish normal/build/development dependencies.
- Model loading and request start still need the transactional rollback work specified by Phase 2.
- Normal shutdown relies on callers invoking explicit bounded shutdown; frontend closure coverage remains incomplete.
- E1 exposes configuration capacity below it but represents only one loaded model generation.
- Candle’s upstream cache and GGUF’s native execution do not support a repository-wide allocation-free backend claim.
- Real-model smoke testing is target-machine work and is not part of the baseline command above.
- GPU execution, remote/browser transport, general chat, GGUF UI selection, and multi-model E1 state are unsupported.

## Historical implementation record

The recovered [implementation plan](implementation-plan.md) is retained as historical context and is not authoritative. The execution plan supersedes its old phase sequence and proposed repository shape.

# Current Implementation Status

**Status date:** 2026-07-23

**Source baseline:** uncommitted Phase 1 working tree based on commit `20fa777a35021e6f80c88542e7e02015c47b8f65`

**Execution position:** Phase 1 quality gate implemented and validated locally; Phase 2 has not started

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
- Typed, fail-closed architecture and external-dependency validation with full layer-matrix and real-workspace integration coverage.
- Linux CI definitions for locked Rust checks, dependency/license/advisory policy, Markdown links, and named portability targets.

Component details are indexed by the [documentation map](../README.md).

## Reproducible validation evidence

Local validation ran on 2026-07-23 from the uncommitted Phase 1 working tree based on commit `20fa777a35021e6f80c88542e7e02015c47b8f65`. A remote CI run URL cannot exist until the change is pushed; `.github/workflows/quality.yml` is configured to run on every push and pull request.

Toolchain and installed targets:

```text
rustc 1.96.1 (31fca3adb 2026-06-26)
cargo 1.96.1 (356927216 2026-06-26)
x86_64-unknown-linux-gnu
wasm32-unknown-unknown
thumbv7em-none-eabihf
```

The complete local runner passed:

```text
cargo run --locked --bin llm-app -- verify
```

It ran typed architecture/dependency validation, formatting, `cargo check --workspace --all-targets --locked`, ordinary `cargo test --workspace --locked`, Clippy with `-D warnings`, API documentation, and `cargo bench --workspace --no-run --locked`. Ordinary tests did not select the Criterion benchmark target; benchmark targets compiled separately.

Additional CI-equivalent checks passed:

```text
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
cargo check --locked --target wasm32-unknown-unknown --lib -p domain-contracts -p tokenization -p context-planner -p sampling -p task-graph
cargo check --locked --target thumbv7em-none-eabihf --lib -p domain-contracts -p tokenization -p context-planner -p sampling -p task-graph
cargo deny --workspace --locked check advisories bans licenses sources
lychee --config lychee.toml --offline '**/*.md'
```

Results:

- 90 ordinary tests passed; zero failed;
- Clippy and rustdoc completed with zero warnings under `-D warnings`;
- all workspace benchmark targets compiled;
- both named non-host library target checks passed for all five portable feature crates;
- the full workspace graph passed `cargo-deny 0.20.2` advisories, dependency bans, licenses, and sources with five documented transitive advisory exceptions;
- Lychee 0.24.2 checked 72 Markdown links: 62 valid, 10 external/offline exclusions, zero errors;
- `cargo tree -d --locked` remains an audit report: metadata contains 57 duplicated package names spanning 120 package-version entries; no blanket deduplication is required;
- `desktop-slint` release binary size was 46,540,808 bytes for `x86_64-unknown-linux-gnu` using the default release profile.

No product binary was launched and no real external model was exercised by this quality-gate validation.

## Known limitations

- The CI workflow is present, but this uncommitted working tree has no remote CI run URL yet; required-branch protection must be configured in the repository host after the workflow is pushed.
- CI currently names Ubuntu 24.04 / `x86_64-unknown-linux-gnu` as the host platform. Windows and macOS jobs remain deferred until native toolchains are documented.
- Scheduled external-link checks depend on third-party site availability; pull-request link checks intentionally validate repository-local links offline.
- Model loading and request start still need the transactional rollback work specified by Phase 2.
- Normal shutdown relies on callers invoking explicit bounded shutdown; frontend closure coverage remains incomplete.
- E1 exposes configuration capacity below it but represents only one loaded model generation.
- Candle’s upstream cache and GGUF’s native execution do not support a repository-wide allocation-free backend claim.
- Real-model smoke testing is target-machine work and is not part of the baseline command above.
- GPU execution, remote/browser transport, general chat, GGUF UI selection, and multi-model E1 state are unsupported.

## Historical implementation record

The recovered [implementation plan](implementation-plan.md) is retained as historical context and is not authoritative. The execution plan supersedes its old phase sequence and proposed repository shape.

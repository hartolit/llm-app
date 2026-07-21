# Engine crates

Runtime and orchestration owners belong here. Engines consume feature and adapter
crates but are never imported by either lower layer.

Current engine tiers:

- E0 `inference-runtime`: exclusive model registry, admission control, request
  lifecycle, cancellation, bounded draining, and deterministic unload;
- E1 `application-runtime`: frontend-neutral Hub resolution, tokenizer
  validation, persistence, and model lifecycle use cases over E0.

The only permitted engine-to-engine edge is
`application-runtime -> inference-runtime`. Adding another engine requires
architectural review and must preserve the consolidated 1–3 crate limit.

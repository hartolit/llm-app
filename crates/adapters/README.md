# Adapter crates

Standard-library, FFI, storage, network, device, and third-party integrations
are quarantined here.

Current adapters:

- `candle-backend`: CPU Llama reference backend using Candle and Safetensors;
- `host-runtime`: bounded host channels, threads, monotonic time, and frame-pull
  output accumulation;
- `hf-tokenizer`: Hugging Face tokenizer implementation of portable tokenizer
  contracts;
- `hf-hub-adapter`: synchronous cached model-artifact resolution;
- `redb-storage`: versioned desktop settings and model-catalogue persistence.

Adapters may depend downward on feature contracts. Features never depend on
adapters, and adapters do not import one another. Outer applications compose
multiple adapters when required.

## `gguf-backend`

Local GGUF CPU inference through quarantined llama.cpp bindings. It owns native
model/context/cache state and exposes only `domain-contracts` types.

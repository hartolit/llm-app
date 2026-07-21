# candle-backend

Phase 3 CPU reference adapter for unquantized Hugging Face Llama models stored as Safetensors.

The crate owns all Candle-specific types and implements only the portable contracts from `domain-contracts`. It supports inspection, admission planning, loading, independent sequence caches, prompt prefill, incremental decode, synchronization, and unload preparation.

## Allocation contract

This adapter does **not** advertise `CapabilitySet::ALLOCATION_FREE_HOT_PATH`. Candle 0.11's upstream Llama implementation concatenates KV-cache tensors as generation advances and creates tensors for forward operations. The adapter remains useful as a correctness and compatibility backend, while a future strict backend must provide pre-allocated cache and execution arenas before claiming the capability.

## Sequence reset

The upstream cache cannot be cleared in place without constructing a replacement. The adapter therefore does not advertise `CapabilitySet::SEQUENCE_RESET`; destroy and recreate the sequence at a cold lifecycle boundary.

## Supported scope

- CPU execution
- unquantized Llama-family models
- Hugging Face `config.json`
- one or more Safetensors shards
- F32, F16, and BF16 requested execution types

CUDA, Metal, quantized GGUF, model downloading, and tokenizer integration remain separate adapters.

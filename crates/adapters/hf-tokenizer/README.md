# hf-tokenizer

Adapter from Hugging Face `tokenizers` to the portable `tokenization` feature.
Generation uses a request-local stateful decoder because whitespace and byte
fallback can depend on surrounding token identifiers.

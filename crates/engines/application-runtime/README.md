# application-runtime

Frontend-neutral orchestration for desktop model acquisition, validation,
persistence, loading, unloading, and bounded worker shutdown.

The crate owns no UI toolkit types. Slint, Tauri, CLI, or another host frontend
can drive the same commands, poll the same structured events, and inspect the
same application state without duplicating Hub, storage, tokenizer, or inference
lifecycle logic.

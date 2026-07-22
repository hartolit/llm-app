# application-runtime

Frontend-neutral orchestration for desktop model acquisition, validation,
persistence, loading, unloading, bounded worker shutdown, and typed corrective
workflows.

The crate owns no UI toolkit types. Slint, Tauri, CLI, or another host frontend
can drive the same commands, poll the same structured events, and inspect the
same application state without duplicating Hub, storage, tokenizer, or inference
lifecycle logic.

Phase 7 adds a statically dispatched `CorrectiveWorkflowExecutor` for the canonical
draft → validate → normalize → review → revise → validate flow. Workflow tasks
exchange immutable `ArtifactId` references instead of copied transcripts; concrete
model and validator services remain injected coarse boundaries.

# Application crates

Applications are thin execution boundaries. They own event loops,
environment-specific paths, presentation, and process exit behavior. Reusable
Hub, persistence, tokenizer, and model lifecycle workflows belong in
`application-runtime`, not in individual frontends.

Current application:

- `desktop-slint`: Slint component construction, callback mapping, frame polling,
  and presentation over `application-runtime`.

A future Tauri or CLI runner should depend on `application-runtime` rather than
copying Candle, Hugging Face, redb, or bounded-worker composition. Lower layers
never import application or Slint types.

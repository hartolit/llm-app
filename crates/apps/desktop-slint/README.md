# desktop-slint

Thin Slint presentation adapter over `application-runtime`.

The crate owns the Slint event loop, per-user database path, widget callbacks,
16 millisecond event polling, and status presentation. It does not directly
import Candle, Hugging Face, redb, Flume, tokenizer, or inference command types.
Those reusable use cases are owned by the E1 application engine.

The binary entry point delegates to the library so process startup remains lean.

Run with:

```text
cargo run -p desktop-slint
```

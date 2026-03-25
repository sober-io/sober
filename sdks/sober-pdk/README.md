# sober-pdk

Guest-side SDK for writing [Sober](https://github.com/sober-io/sober) plugins as WebAssembly modules.

## Quick Start

```rust
use sober_pdk::{plugin_fn, FnResult, Json};

#[plugin_fn]
pub fn handle(input: Json<serde_json::Value>) -> FnResult<Json<serde_json::Value>> {
    let name = input.0["name"].as_str().unwrap_or("world");
    Ok(Json(serde_json::json!({ "greeting": format!("Hello, {name}!") })))
}
```

## Capabilities

Plugins declare capabilities in `plugin.toml` and access them through typed modules:

| Module | Capability | Description |
|--------|-----------|-------------|
| `sober_pdk::log` | *(always)* | Structured logging |
| `sober_pdk::kv` | `key_value` | Persistent key-value storage |
| `sober_pdk::http` | `network` | Outbound HTTP requests |
| `sober_pdk::secret` | `secret_read` | Read-only secret access |
| `sober_pdk::tool` | `tool_call` | Invoke other registered tools |
| `sober_pdk::metrics` | `metrics` | Emit counters, gauges, histograms |
| `sober_pdk::memory` | `memory_read` / `memory_write` | Vector memory search and storage |
| `sober_pdk::conversation` | `conversation_read` | Read conversation history |
| `sober_pdk::schedule` | `schedule` | Create deferred or recurring jobs |
| `sober_pdk::fs` | `filesystem` | Sandboxed file read/write |
| `sober_pdk::llm` | `llm_call` | LLM text completions |

## License

MIT

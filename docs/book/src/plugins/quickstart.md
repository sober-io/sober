# Quick Start

This tutorial walks through building, configuring, and installing a minimal Sõber
WASM plugin that exposes a single tool.

## Prerequisites

- Rust toolchain with the `wasm32-wasip1` target
- Access to the Sõber HTTP API or settings UI

```bash
rustup target add wasm32-wasip1
```

## Step 1 — Create a Rust library project

```bash
cargo new --lib my-plugin
cd my-plugin
```

## Step 2 — Add the sober-pdk dependency

Open `Cargo.toml` and add:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
sober-pdk = "0.1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

The `cdylib` crate type tells the compiler to produce a dynamic library — the
format WASM plugins use.

## Step 3 — Write a tool function

Replace `src/lib.rs` with:

```rust
use sober_pdk::{plugin_fn, FnResult, Json};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct GreetInput {
    name: String,
}

#[derive(Serialize)]
struct GreetOutput {
    message: String,
}

#[plugin_fn]
pub fn greet(input: Json<GreetInput>) -> FnResult<Json<GreetOutput>> {
    let output = GreetOutput {
        message: format!("Hello, {}! Greetings from my-plugin.", input.0.name),
    };
    Ok(Json(output))
}
```

The `#[plugin_fn]` attribute marks the function as a tool export. The function
name (`greet`) must match the `name` field in the manifest's `[[tools]]` entry.

### Optional: add a self-test

Export a `__sober_test` function and the audit pipeline will call it during
installation. If it returns an error the plugin is rejected.

```rust
#[plugin_fn]
pub fn __sober_test(_: ()) -> FnResult<()> {
    // Basic sanity check — exercise the happy path.
    let result = greet(Json(GreetInput { name: "test".into() }))?;
    assert!(!result.0.message.is_empty());
    Ok(())
}
```

## Step 4 — Create `plugin.toml`

Place this file in the project root alongside `Cargo.toml`:

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "A minimal greeting plugin"

[[tools]]
name = "greet"
description = "Returns a greeting for the given name"
```

This minimal manifest declares no capabilities — the plugin only needs the
always-available logging functions.

## Step 5 — Build

```bash
cargo build --target wasm32-wasip1 --release
```

The compiled module is at:

```
target/wasm32-wasip1/release/my_plugin.wasm
```

## Step 6 — Install

Plugins are installed via the Sõber settings UI or the HTTP API. The API accepts
a JSON payload that includes the base64-encoded WASM binary and the manifest
contents:

```bash
curl -X POST http://localhost:8080/api/v1/plugins \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "wasm_b64": "'$(base64 -w0 target/wasm32-wasip1/release/my_plugin.wasm)'",
    "manifest": "'"$(cat plugin.toml)"'"
  }'
```

A successful install returns the plugin ID and an audit report. If any audit
stage fails the response includes the rejection reason.

## Verify

Ask Sõber to greet someone. The agent will discover the new `greet` tool and
can invoke it when appropriate, or you can call it directly:

```bash
sober tool call greet '{"name": "world"}'
# {"message": "Hello, world! Greetings from my-plugin."}
```

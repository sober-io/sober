# Plugin Examples

## Example 1 — Simple greeting tool (no capabilities)

A minimal plugin that exposes a single tool with no host capabilities. It
demonstrates the basic structure: a `plugin.toml` manifest and a `#[plugin_fn]`
Rust function.

**`plugin.toml`**

```toml
[plugin]
name        = "greeter"
version     = "0.1.0"
description = "Returns a personalised greeting"

[[tools]]
name        = "greet"
description = "Returns a greeting for the given name and optional language"
```

**`src/lib.rs`**

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct GreetInput {
    name: String,
    #[serde(default = "default_lang")]
    language: String,
}

fn default_lang() -> String {
    "en".into()
}

#[derive(Serialize)]
struct GreetOutput {
    message: String,
}

#[plugin_fn]
pub fn greet(Json(input): Json<GreetInput>) -> FnResult<Json<GreetOutput>> {
    let message = match input.language.as_str() {
        "et" => format!("Tere, {}!", input.name),
        "es" => format!("Hola, {}!", input.name),
        "fr" => format!("Bonjour, {}!", input.name),
        _    => format!("Hello, {}!", input.name),
    };
    Ok(Json(GreetOutput { message }))
}

// Optional self-test called by the audit pipeline.
#[plugin_fn]
pub fn __sober_test(_: ()) -> FnResult<()> {
    let out = greet(Json(GreetInput {
        name: "World".into(),
        language: "en".into(),
    }))?;
    assert!(!out.message.is_empty());
    Ok(())
}
```

**Build**

```bash
cargo build --target wasm32-wasip1 --release
```

---

## Example 2 — Web scraper (network capability)

A plugin that fetches a web page and returns its plain-text content. It
demonstrates the `network` capability with domain restriction and the use of
`host_log` for diagnostics.

**`plugin.toml`**

```toml
[plugin]
name        = "web-scraper"
version     = "0.2.0"
description = "Fetches and returns the text content of a web page"

[capabilities]
network = { allowed_hosts = ["en.wikipedia.org", "www.bbc.co.uk"] }

[[tools]]
name        = "fetch_page"
description = "Fetches the raw HTML of a URL and returns it as text"
```

**`src/lib.rs`**

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
struct FetchInput {
    url: String,
}

#[derive(Serialize)]
struct FetchOutput {
    status: u16,
    body:   String,
}

fn log(level: &str, message: &str) {
    let req = json!({ "level": level, "message": message, "fields": {} });
    host::call("sober", "host_log", &serde_json::to_vec(&req).unwrap()).ok();
}

fn http_get(url: &str) -> Result<(u16, String), String> {
    let req = json!({
        "method":  "GET",
        "url":     url,
        "headers": { "User-Agent": "sober-plugin/web-scraper" },
        "body":    null
    });
    let raw = host::call("sober", "host_http_request", &serde_json::to_vec(&req).unwrap())
        .map_err(|e| e.to_string())?;
    let resp: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;

    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_owned());
    }
    let status = resp["status"].as_u64().unwrap_or(0) as u16;
    let body   = resp["body"].as_str().unwrap_or("").to_owned();
    Ok((status, body))
}

#[plugin_fn]
pub fn fetch_page(Json(input): Json<FetchInput>) -> FnResult<Json<FetchOutput>> {
    log("info", &format!("fetching {}", input.url));

    match http_get(&input.url) {
        Ok((status, body)) => {
            log("info", &format!("got status {status}"));
            Ok(Json(FetchOutput { status, body }))
        }
        Err(e) => {
            log("error", &format!("fetch failed: {e}"));
            Err(Error::msg(e))
        }
    }
}
```

---

## Example 3 — Knowledge indexer (memory_read + memory_write)

A plugin that accepts a text document, stores it in vector memory, and exposes
a search tool that queries previously indexed content. Demonstrates both memory
capabilities and the KV store for tracking indexed document IDs.

**`plugin.toml`**

```toml
[plugin]
name        = "knowledge-indexer"
version     = "0.1.0"
description = "Indexes text documents into vector memory and enables semantic search"

[capabilities]
memory_read  = { scopes = ["user"] }
memory_write = { scopes = ["user"] }
key_value    = true

[[tools]]
name        = "index_document"
description = "Stores a text document in vector memory for later retrieval"

[[tools]]
name        = "search_knowledge"
description = "Performs a semantic search over previously indexed documents"
```

**`src/lib.rs`**

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// Helper: call a host function and parse JSON response.
// ---------------------------------------------------------------------------

fn call_host(fn_name: &str, req: &serde_json::Value) -> Result<serde_json::Value, Error> {
    let raw = host::call("sober", fn_name, &serde_json::to_vec(req).unwrap())?;
    Ok(serde_json::from_slice(&raw)?)
}

// ---------------------------------------------------------------------------
// Tool: index_document
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct IndexInput {
    content: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Serialize)]
struct IndexOutput {
    indexed: bool,
}

#[plugin_fn]
pub fn index_document(Json(input): Json<IndexInput>) -> FnResult<Json<IndexOutput>> {
    let mut metadata = json!({});
    if let Some(src) = &input.source {
        metadata["source"] = serde_json::Value::String(src.clone());
    }

    let write_req = json!({
        "content":  input.content,
        "scope":    "user",
        "metadata": metadata
    });

    let resp = call_host("host_memory_write", &write_req)?;

    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(Error::msg(format!("memory write failed: {err}")));
    }

    // Track indexed count in KV store.
    let count_resp = call_host("host_kv_get", &json!({ "key": "indexed_count" }))?;
    let count: u64 = count_resp["value"]
        .as_u64()
        .unwrap_or(0);

    call_host("host_kv_set", &json!({ "key": "indexed_count", "value": count + 1 }))?;

    Ok(Json(IndexOutput { indexed: true }))
}

// ---------------------------------------------------------------------------
// Tool: search_knowledge
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchInput {
    query: String,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    5
}

#[derive(Serialize)]
struct SearchResult {
    content: String,
    score:   f64,
}

#[derive(Serialize)]
struct SearchOutput {
    results: Vec<SearchResult>,
}

#[plugin_fn]
pub fn search_knowledge(Json(input): Json<SearchInput>) -> FnResult<Json<SearchOutput>> {
    let query_req = json!({
        "query": input.query,
        "scope": "user",
        "limit": input.limit
    });

    let resp = call_host("host_memory_query", &query_req)?;

    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(Error::msg(format!("memory query failed: {err}")));
    }

    let results = resp["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|r| SearchResult {
                    content: r["content"].as_str().unwrap_or("").to_owned(),
                    score:   r["score"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(SearchOutput { results }))
}
```

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

**`Cargo.toml`**

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
sober-pdk = "0.1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**`src/lib.rs`**

```rust
use sober_pdk::{plugin_fn, FnResult, Json};
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
demonstrates the `network` capability with domain restriction and structured
logging.

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

**`Cargo.toml`**

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
sober-pdk = "0.1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**`src/lib.rs`**

```rust
use sober_pdk::{plugin_fn, FnResult, Json, log, http};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct FetchInput {
    url: String,
}

#[derive(Serialize)]
struct FetchOutput {
    status: u16,
    body:   String,
}

#[plugin_fn]
pub fn fetch_page(Json(input): Json<FetchInput>) -> FnResult<Json<FetchOutput>> {
    log::info(&format!("fetching {}", input.url));

    let resp = http::get(
        &input.url,
        &[("User-Agent", "sober-plugin/web-scraper")],
    );

    match resp {
        Ok(r) => {
            log::info(&format!("got status {}", r.status));
            Ok(Json(FetchOutput { status: r.status, body: r.body }))
        }
        Err(e) => {
            log::error(&format!("fetch failed: {e}"));
            Err(e)
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

**`Cargo.toml`**

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
sober-pdk = "0.1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**`src/lib.rs`**

```rust
use sober_pdk::{plugin_fn, FnResult, Json, kv, memory, log};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    let mut metadata = HashMap::new();
    if let Some(src) = &input.source {
        metadata.insert("source".to_string(), serde_json::json!(src));
    }

    memory::write(&input.content, Some("user"), metadata)?;

    // Track indexed count in KV store.
    let count = kv::get("indexed_count")?
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    kv::set("indexed_count", &serde_json::json!(count + 1))?;

    log::info(&format!("indexed document #{}", count + 1));

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
    let hits = memory::query(&input.query, Some("user"), Some(input.limit))?;

    let results = hits
        .into_iter()
        .map(|hit| SearchResult {
            content: hit.content,
            score:   hit.score,
        })
        .collect();

    Ok(Json(SearchOutput { results }))
}
```

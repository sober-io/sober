//! LLM-powered plugin generation with a self-correcting retry loop.
//!
//! [`PluginGenerator`] drives a two-phase loop:
//! 1. Ask the LLM to write Rust source code for a WASM plugin.
//! 2. Compile the result; if compilation fails, feed the error back to the LLM
//!    and retry (up to [`PluginGenerator::max_retries`] times).
//!
//! Skill generation is simpler — no compilation step — so it only calls the LLM
//! once and returns the markdown content.

use std::sync::Arc;

use sober_llm::{CompletionRequest, LlmEngine, Message};
use tokio::fs;

use crate::{GenError, compile::compile};

// ---------------------------------------------------------------------------
// Generation constants
// ---------------------------------------------------------------------------

/// Maximum tokens for WASM plugin generation. Plugins can be hundreds of
/// lines with imports, structs, multiple functions, and error handling.
/// 16384 provides headroom for complex plugins without truncation.
const WASM_GEN_MAX_TOKENS: u32 = 16_384;

/// Maximum tokens for skill generation. Skills are markdown documents
/// that can include detailed instructions, examples, and checklists.
const SKILL_GEN_MAX_TOKENS: u32 = 8192;

/// Low temperature for WASM generation — deterministic code output is
/// preferred over creative variation to minimize compile failures.
const WASM_GEN_TEMPERATURE: f32 = 0.2;

/// Slightly higher temperature for skill generation — skills benefit from
/// more natural language variety while remaining structured.
const SKILL_GEN_TEMPERATURE: f32 = 0.3;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The result of a successful WASM plugin generation.
#[derive(Debug, Clone)]
pub struct GeneratedPlugin {
    /// Compiled WASM bytes ready for execution.
    pub wasm_bytes: Vec<u8>,
    /// `plugin.toml` manifest content.
    pub manifest: String,
    /// `src/lib.rs` source content returned by the LLM.
    pub source: String,
}

/// Drives LLM-powered plugin and skill generation.
///
/// Wraps an [`LlmEngine`] and orchestrates the prompt → generate → compile →
/// retry loop for WASM plugins, and a simple prompt → generate loop for
/// markdown skills.
pub struct PluginGenerator {
    llm: Arc<dyn LlmEngine>,
    model: String,
    /// Maximum number of LLM calls per generation request (initial + retries).
    pub max_retries: u32,
}

impl PluginGenerator {
    /// Create a new `PluginGenerator`.
    ///
    /// `max_retries` defaults to 3 (initial attempt + up to 2 follow-ups).
    pub fn new(llm: Arc<dyn LlmEngine>, model: impl Into<String>) -> Self {
        Self {
            llm,
            model: model.into(),
            max_retries: 3,
        }
    }

    /// Generate a WASM plugin from a natural-language description.
    ///
    /// Workflow:
    /// 1. Ask the LLM for Rust source that implements the plugin.
    /// 2. Write the source to a temporary directory alongside a `Cargo.toml`
    ///    and `plugin.toml`.
    /// 3. Compile with `cargo build --target wasm32-wasip1 --release`.
    /// 4. On compile failure, send the error back to the LLM and retry.
    /// 5. Return the compiled bytes, manifest, and source on success.
    ///
    /// # Errors
    ///
    /// - [`GenError::Generate`] — LLM returned unusable output or max retries
    ///   were exhausted.
    /// - [`GenError::Compile`] — Compilation failed on the last attempt.
    /// - [`GenError::Io`] — File system error while writing temp files.
    pub async fn generate_wasm(
        &self,
        name: &str,
        description: &str,
        capabilities: &[String],
    ) -> Result<GeneratedPlugin, GenError> {
        let dir = tempfile::tempdir()?;
        let src_dir = dir.path();

        // Write the fixed project skeleton (Cargo.toml + plugin.toml).
        let manifest = plugin_toml(name, description, capabilities);
        fs::write(src_dir.join("plugin.toml"), &manifest).await?;
        fs::write(src_dir.join("Cargo.toml"), cargo_toml(name)).await?;
        fs::create_dir_all(src_dir.join("src")).await?;

        // Build the initial message list.
        let system_prompt = wasm_system_prompt();
        let user_prompt = wasm_user_prompt(name, description, capabilities);

        let mut messages = vec![Message::system(&system_prompt), Message::user(&user_prompt)];

        let mut last_error: Option<String> = None;

        for attempt in 0..self.max_retries {
            let req = CompletionRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: vec![],
                max_tokens: Some(WASM_GEN_MAX_TOKENS),
                temperature: Some(WASM_GEN_TEMPERATURE),
                stop: vec![],
                stream: false,
            };

            let response = self
                .llm
                .complete(req)
                .await
                .map_err(|e| GenError::Generate(e.to_string()))?;

            let raw = response
                .choices
                .first()
                .and_then(|c| c.message.content.as_deref())
                .ok_or_else(|| GenError::Generate("LLM returned no content".to_string()))?;

            let extracted = extract_rust_source(raw);
            let source = if extracted.is_empty() {
                tracing::warn!(
                    attempt,
                    response_len = raw.len(),
                    response_prefix = &raw[..raw.len().min(200)],
                    "LLM response did not contain a fenced Rust code block"
                );
                // If the response looks like Rust source (has use/fn keywords),
                // use it directly — some models omit the code fence.
                if raw.contains("use ") || raw.contains("fn ") {
                    tracing::info!("treating unfenced response as raw Rust source");
                    raw.trim().to_string()
                } else {
                    return Err(GenError::Generate(
                        "LLM did not return a Rust code block".to_string(),
                    ));
                }
            } else {
                extracted
            };

            fs::write(src_dir.join("src").join("lib.rs"), &source).await?;

            match compile(src_dir).await {
                Ok(wasm_bytes) => {
                    return Ok(GeneratedPlugin {
                        wasm_bytes,
                        manifest,
                        source,
                    });
                }
                Err(GenError::Compile(err)) => {
                    tracing::warn!(
                        attempt,
                        plugin = name,
                        error = %err,
                        "WASM compile failed; feeding error back to LLM"
                    );
                    last_error = Some(err.clone());

                    // Add assistant message + user error feedback to the history.
                    messages.push(Message::assistant(raw));
                    messages.push(Message::user(compile_error_prompt(&err)));
                }
                Err(other) => return Err(other),
            }
        }

        Err(GenError::Generate(format!(
            "Exhausted {} retries. Last compile error: {}",
            self.max_retries,
            last_error.as_deref().unwrap_or("unknown")
        )))
    }

    /// Generate a markdown skill definition from a natural-language description.
    ///
    /// A "skill" is a reusable prompt template stored as markdown. Unlike WASM
    /// plugins there is no compilation step — the LLM response is returned as-is.
    ///
    /// # Errors
    ///
    /// - [`GenError::Generate`] — LLM call failed or returned empty content.
    pub async fn generate_skill(&self, name: &str, description: &str) -> Result<String, GenError> {
        let req = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message::system(skill_system_prompt()),
                Message::user(skill_user_prompt(name, description)),
            ],
            tools: vec![],
            max_tokens: Some(SKILL_GEN_MAX_TOKENS),
            temperature: Some(SKILL_GEN_TEMPERATURE),
            stop: vec![],
            stream: false,
        };

        let response = self
            .llm
            .complete(req)
            .await
            .map_err(|e| GenError::Generate(e.to_string()))?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| GenError::Generate("LLM returned empty skill content".to_string()))?;

        Ok(content)
    }
}

// ---------------------------------------------------------------------------
// Prompt builders
// ---------------------------------------------------------------------------

fn wasm_system_prompt() -> String {
    r#"You are an expert Rust developer who writes Sõber WASM plugins.

A Sõber WASM plugin is a Rust `cdylib` crate that uses the `extism-pdk` and
`sober-pdk` crates. Each exported tool function must be annotated with
`#[plugin_fn]` and have the signature:

```rust
#[plugin_fn]
pub fn tool_name(input: String) -> FnResult<String>
```

The input is a JSON string. The return value must also be a JSON string.

Available capabilities via `sober_pdk`:
- `sober_pdk::log::info(msg)` / `warn(msg)` / `error(msg)` — structured logging (always available)
- `sober_pdk::kv::get(key)` / `set(key, value)` / `delete(key)` — key-value store (requires `key_value` capability)
- `sober_pdk::http::fetch(url, method, headers, body)` — HTTP requests (requires `network` capability)
- `sober_pdk::secrets::get(key)` — read a secret (requires `secret_read` capability)
- `sober_pdk::tool_call::invoke(name, input)` — call other tools/plugins (requires `tool_call` capability)
- `sober_pdk::metrics::counter(name)` / `gauge(name, value)` — emit metrics (requires `metrics` capability)
- `sober_pdk::memory::read(scope, query)` — read from memory/context (requires `memory_read` capability; not yet connected)
- `sober_pdk::memory::write(scope, data)` — write to memory/context (requires `memory_write` capability; not yet connected)
- `sober_pdk::conversation::read(id)` — read conversation history (requires `conversation_read` capability; not yet connected)
- `sober_pdk::schedule::create(spec)` — create scheduled jobs (requires `schedule` capability; not yet connected)
- `sober_pdk::fs::read(path)` / `write(path, data)` — filesystem access (requires `filesystem` capability; not yet connected)
- `sober_pdk::llm::call(prompt)` — invoke an LLM provider (requires `llm_call` capability; not yet connected)

Manifest format (`plugin.toml`):
```toml
[plugin]
name = "plugin-name"
version = "0.1.0"
description = "..."

[capabilities]
key_value = true     # optional
network = true       # optional
secret_read = true   # optional

[[tools]]
name = "tool-name"
description = "..."
```

## Full example: a plugin using logging, HTTP, key-value, and secrets

```rust
use extism_pdk::*;
use serde_json::{json, Value};
use sober_pdk::{http, kv, log, secrets};

/// Fetches weather data for a city, caches it, and returns the result.
#[plugin_fn]
pub fn weather(input: String) -> FnResult<String> {
    // Parse JSON input
    let params: Value = serde_json::from_str(&input)?;
    let city = params["city"]
        .as_str()
        .ok_or_else(|| Error::msg("missing 'city' field"))?;

    log::info(&format!("weather lookup for: {city}"));

    // Check cache first (key_value capability)
    let cache_key = format!("weather:{city}");
    if let Some(cached) = kv::get(&cache_key)? {
        log::info("cache hit");
        return Ok(cached);
    }

    // Read API key from secrets (secret_read capability)
    let api_key = secrets::get("WEATHER_API_KEY")?
        .ok_or_else(|| Error::msg("WEATHER_API_KEY secret not configured"))?;

    // Fetch from API (network capability)
    let url = format!("https://api.weather.example/v1?city={city}&key={api_key}");
    let response = http::get(&url)?;

    if response.status != 200 {
        log::error(&format!("API returned status {}", response.status));
        return Ok(json!({
            "error": format!("API error: HTTP {}", response.status)
        }).to_string());
    }

    let result = json!({
        "city": city,
        "data": serde_json::from_str::<Value>(&response.body)
            .unwrap_or(json!({"raw": response.body}))
    }).to_string();

    // Cache for next time (key_value capability)
    kv::set(&cache_key, &result)?;

    Ok(result)
}
```

The corresponding `plugin.toml` for this example:
```toml
[plugin]
name = "weather"
version = "0.1.0"
description = "Fetches and caches weather data"

[capabilities]
key_value = true
network = true
secret_read = true

[[tools]]
name = "weather"
description = "Look up weather for a city"
```

Rules:
- Output ONLY a single fenced Rust code block (```rust ... ```).
- Do NOT include Cargo.toml or plugin.toml — those are provided separately.
- Use `serde_json` to parse inputs and build outputs.
- Handle errors gracefully; return a JSON error object on failure.
- No `unsafe` blocks.
- No `.unwrap()` — use `?` or explicit error handling.
- Follow the example above as a structural template."#
        .to_string()
}

fn wasm_user_prompt(name: &str, description: &str, capabilities: &[String]) -> String {
    let caps = if capabilities.is_empty() {
        "none".to_string()
    } else {
        capabilities.join(", ")
    };

    format!(
        "Generate a Sõber WASM plugin.\n\nPlugin name: {name}\nDescription: {description}\nRequired capabilities: {caps}\n\nRespond with only a ```rust code block containing src/lib.rs."
    )
}

fn compile_error_prompt(error: &str) -> String {
    format!(
        "The code you provided failed to compile. Please fix the errors and respond with the corrected ```rust code block only.\n\nCompiler output:\n```\n{error}\n```"
    )
}

fn plugin_toml(name: &str, description: &str, capabilities: &[String]) -> String {
    let mut lines = vec![
        format!("[plugin]"),
        format!("name = \"{name}\""),
        format!("version = \"0.1.0\""),
        format!("description = \"{description}\""),
        String::new(),
        "[capabilities]".to_string(),
    ];

    for cap in capabilities {
        lines.push(format!("{cap} = true"));
    }

    lines.push(String::new());
    lines.push("[[tools]]".to_string());
    lines.push(format!("name = \"{name}\""));
    lines.push(format!("description = \"{description}\""));

    lines.join("\n")
}

fn cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "sober-plugin-{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
    )
}

fn skill_system_prompt() -> &'static str {
    r#"You are an expert at writing Sõber skill definitions.

A Sõber skill is a reusable prompt template stored as a markdown document. It
instructs the agent how to handle a specific type of task.

Structure:
```markdown
# Skill: <Name>

## Description
<One-sentence summary>

## When to use
<Bullet list of triggers / use cases>

## Instructions
<Step-by-step guidance for the agent>

## Output format
<Expected output structure>

## Example
<A worked example>
```

Rules:
- Be concise and precise.
- Use imperative language in the Instructions section.
- The Output format section must describe a structured format (JSON, markdown table, etc.).
- Do not include implementation code — skills are prompt templates, not programs."#
}

fn skill_user_prompt(name: &str, description: &str) -> String {
    format!(
        "Generate a Sõber skill definition.\n\nSkill name: {name}\nDescription: {description}\n\nRespond with the full markdown skill document."
    )
}

// ---------------------------------------------------------------------------
// Source extraction
// ---------------------------------------------------------------------------

/// Extract the content of the first ```rust ... ``` block from the LLM response.
///
/// Returns an empty string if no fenced Rust block is found.
fn extract_rust_source(raw: &str) -> String {
    // Look for ```rust or ``` followed immediately by a newline.
    let fence_start = raw
        .find("```rust")
        .or_else(|| raw.find("```\n"))
        .or_else(|| raw.find("``` \n"));

    let Some(start) = fence_start else {
        return String::new();
    };

    // Skip past the opening fence line.
    let after_fence = &raw[start..];
    let Some(newline) = after_fence.find('\n') else {
        return String::new();
    };
    let code_start = start + newline + 1;

    // Find closing fence.
    let remaining = &raw[code_start..];
    let end = remaining.find("\n```").or_else(|| remaining.find("```"));

    match end {
        Some(e) => raw[code_start..code_start + e].trim().to_string(),
        None => remaining.trim().to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use futures::Stream;
    use sober_llm::{
        CompletionRequest, CompletionResponse, EngineCapabilities, LlmEngine, LlmError, Message,
        types::StreamChunk,
    };

    use super::*;

    // -----------------------------------------------------------------------
    // Mock LLM engine
    // -----------------------------------------------------------------------

    /// A mock LLM engine that replays pre-canned responses in order.
    struct MockLlm {
        /// Each item is either `Ok(content)` or `Err(message)`.
        responses: Mutex<Vec<Result<String, String>>>,
        /// Captures each request for assertion.
        calls: Mutex<Vec<CompletionRequest>>,
    }

    impl MockLlm {
        fn new(responses: Vec<Result<String, String>>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(vec![]),
            })
        }

        fn call_count(&self) -> usize {
            self.calls.lock().expect("lock calls").len()
        }

        fn last_messages(&self) -> Vec<Message> {
            self.calls
                .lock()
                .expect("lock calls")
                .last()
                .map(|r| r.messages.clone())
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl LlmEngine for MockLlm {
        async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            self.calls.lock().expect("lock calls").push(req);

            let next = self.responses.lock().expect("lock responses").remove(0);

            match next {
                Ok(content) => Ok(CompletionResponse {
                    id: "mock-id".to_string(),
                    choices: vec![sober_llm::types::Choice {
                        index: 0,
                        message: Message::assistant(content),
                        finish_reason: Some("stop".to_string()),
                    }],
                    usage: None,
                }),
                Err(msg) => Err(LlmError::ApiError {
                    status: 500,
                    message: msg,
                }),
            }
        }

        async fn stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError>
        {
            Err(LlmError::Unsupported("mock does not stream".to_string()))
        }

        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
            Err(LlmError::Unsupported("mock does not embed".to_string()))
        }

        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                supports_tools: false,
                supports_streaming: false,
                supports_embeddings: false,
                max_context_tokens: 8192,
            }
        }

        fn model_id(&self) -> &str {
            "mock/model"
        }
    }

    // -----------------------------------------------------------------------
    // extract_rust_source
    // -----------------------------------------------------------------------

    #[test]
    fn extract_rust_source_finds_fenced_block() {
        let raw = "Here is the code:\n```rust\nfn main() {}\n```\nDone.";
        assert_eq!(extract_rust_source(raw), "fn main() {}");
    }

    #[test]
    fn extract_rust_source_returns_empty_when_no_fence() {
        assert_eq!(extract_rust_source("no code here"), "");
    }

    #[test]
    fn extract_rust_source_handles_unmarked_fence() {
        let raw = "Result:\n```\nlet x = 1;\n```";
        assert_eq!(extract_rust_source(raw), "let x = 1;");
    }

    // -----------------------------------------------------------------------
    // Skill generation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_skill_returns_llm_content() {
        let expected = "# Skill: Summariser\n\nSummarises text.";
        let mock = MockLlm::new(vec![Ok(expected.to_string())]);
        let generator = PluginGenerator::new(mock.clone(), "test-model");

        let result = generator
            .generate_skill("summariser", "Summarise long text into bullet points")
            .await
            .expect("generate_skill failed");

        assert_eq!(result, expected);
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn generate_skill_prompt_contains_name_and_description() {
        let mock = MockLlm::new(vec![Ok("# Skill: Test\n\nContent.".to_string())]);
        let generator = PluginGenerator::new(mock.clone(), "test-model");

        generator
            .generate_skill("weather-lookup", "Look up weather for a city")
            .await
            .expect("generate_skill failed");

        let messages = mock.last_messages();
        // system message + user message
        assert_eq!(messages.len(), 2);
        let user_content = messages[1].content.as_deref().unwrap_or("");
        assert!(
            user_content.contains("weather-lookup"),
            "user prompt missing skill name"
        );
        assert!(
            user_content.contains("Look up weather for a city"),
            "user prompt missing description"
        );
    }

    #[tokio::test]
    async fn generate_skill_propagates_llm_error() {
        let mock = MockLlm::new(vec![Err("provider unavailable".to_string())]);
        let generator = PluginGenerator::new(mock, "test-model");

        let result = generator.generate_skill("test", "test").await;
        assert!(
            matches!(result, Err(GenError::Generate(_))),
            "expected GenError::Generate, got {result:?}"
        );
    }

    #[tokio::test]
    async fn generate_skill_errors_on_empty_content() {
        let mock = MockLlm::new(vec![Ok(String::new())]);
        let generator = PluginGenerator::new(mock, "test-model");

        let result = generator.generate_skill("test", "test").await;
        assert!(
            matches!(result, Err(GenError::Generate(_))),
            "expected GenError::Generate on empty content, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // WASM generation — logic tests (no actual compilation)
    // -----------------------------------------------------------------------

    /// When the LLM fails immediately, we should exhaust retries and return
    /// `GenError::Generate`.
    #[tokio::test]
    async fn generate_wasm_exhausts_retries_on_no_rust_block() {
        // Return responses that have no ```rust block — the generator should
        // exhaust retries.
        let mock = MockLlm::new(vec![
            Ok("I cannot write that code.".to_string()),
            Ok("I still cannot.".to_string()),
            Ok("Sorry.".to_string()),
        ]);
        let mut generator = PluginGenerator::new(mock.clone(), "test-model");
        generator.max_retries = 3;

        let result = generator
            .generate_wasm("test-plugin", "A test plugin", &[])
            .await;

        assert!(
            matches!(result, Err(GenError::Generate(_))),
            "expected GenError::Generate, got {result:?}"
        );
        // Only one call should have been made — we bail early when there's no
        // code block.
        assert_eq!(mock.call_count(), 1);
    }

    /// Verify max_retries=1 means only one attempt.
    #[tokio::test]
    async fn generate_wasm_respects_max_retries_one() {
        let mock = MockLlm::new(vec![Ok("no code".to_string())]);
        let mut generator = PluginGenerator::new(mock.clone(), "test-model");
        generator.max_retries = 1;

        let result = generator.generate_wasm("p", "desc", &[]).await;
        assert!(matches!(result, Err(GenError::Generate(_))));
        assert_eq!(mock.call_count(), 1);
    }

    /// Verify that the initial prompt contains the plugin name, description,
    /// and capabilities.
    #[tokio::test]
    async fn generate_wasm_initial_prompt_contains_metadata() {
        let mock = MockLlm::new(vec![Ok("no code".to_string())]);
        let mut generator = PluginGenerator::new(mock.clone(), "test-model");
        generator.max_retries = 1;

        let caps = vec!["network".to_string(), "key_value".to_string()];
        let _ = generator
            .generate_wasm("my-plugin", "Does something cool", &caps)
            .await;

        let messages = mock.last_messages();
        let user_content = messages[1].content.as_deref().unwrap_or("");
        assert!(user_content.contains("my-plugin"), "missing plugin name");
        assert!(
            user_content.contains("Does something cool"),
            "missing description"
        );
        assert!(user_content.contains("network"), "missing capability");
        assert!(user_content.contains("key_value"), "missing capability");
    }

    // -----------------------------------------------------------------------
    // plugin_toml helper
    // -----------------------------------------------------------------------

    #[test]
    fn plugin_toml_includes_capabilities() {
        let manifest = plugin_toml("weather", "Get weather", &["network".to_string()]);
        assert!(manifest.contains("network = true"));
        assert!(manifest.contains(r#"name = "weather""#));
    }

    #[test]
    fn plugin_toml_no_capabilities_section_is_empty() {
        let manifest = plugin_toml("simple", "Simple plugin", &[]);
        // [capabilities] section exists but has no entries.
        assert!(manifest.contains("[capabilities]"));
        // No keys should follow capabilities before the next section.
        let caps_idx = manifest.find("[capabilities]").unwrap();
        let tools_idx = manifest.find("[[tools]]").unwrap();
        let between = &manifest[caps_idx + "[capabilities]".len()..tools_idx];
        // Between [capabilities] and [[tools]] there should be no `= true` lines.
        assert!(
            !between.contains("= true"),
            "unexpected capability entry: {between}"
        );
    }
}

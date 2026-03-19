//! ACP (Agent Client Protocol) engine for sending prompts through local agents.
//!
//! Spawns an ACP-compatible agent (Claude Code, Kimi Code, Goose) as a
//! subprocess and communicates via JSON-RPC 2.0 over stdin/stdout.

use std::collections::HashMap;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Instant;

use async_trait::async_trait;
use futures::{Stream, stream};
use metrics::{counter, histogram};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::engine::LlmEngine;
use crate::error::LlmError;
use crate::jsonrpc::{JsonRpcNotification, JsonRpcTransport};
use crate::types::{
    Choice, CompletionRequest, CompletionResponse, EngineCapabilities, Message, MessageDelta,
    StreamChoice, StreamChunk, Usage,
};

/// Configuration for an ACP agent.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// Display name for the agent.
    pub name: String,
    /// Binary to spawn (e.g. `"claude"`, `"kimi"`).
    pub command: String,
    /// CLI arguments (e.g. `["acp"]`).
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: HashMap<String, String>,
}

/// Mutable state protected by a Mutex for interior mutability.
struct AcpState {
    child: Option<Child>,
    transport: Option<JsonRpcTransport>,
    session_id: Option<String>,
    model_id_str: String,
}

/// An LLM engine backed by a local ACP-compatible agent subprocess.
///
/// Spawns the agent on first use and reuses the process for subsequent
/// requests. The subprocess is killed when the engine is dropped.
pub struct AcpEngine {
    config: AcpConfig,
    state: Mutex<AcpState>,
}

impl std::fmt::Debug for AcpEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpEngine")
            .field("config", &self.config)
            .finish()
    }
}

impl AcpEngine {
    /// Create a new ACP engine. Does not spawn the subprocess yet — that
    /// happens on the first request.
    pub fn new(config: AcpConfig) -> Self {
        let model_id_str = format!("acp:{}", config.name);
        Self {
            config,
            state: Mutex::new(AcpState {
                child: None,
                transport: None,
                session_id: None,
                model_id_str,
            }),
        }
    }

    /// Return the model ID string. Reads from state under lock.
    pub async fn model_id_async(&self) -> String {
        self.state.lock().await.model_id_str.clone()
    }

    /// Ensure the subprocess is running and initialized.
    async fn ensure_spawned(state: &mut AcpState, config: &AcpConfig) -> Result<(), LlmError> {
        if state.transport.is_some() {
            return Ok(());
        }

        info!(
            command = %config.command,
            args = ?config.args,
            "spawning ACP agent subprocess"
        );

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .envs(&config.env);

        let mut child = cmd.spawn().map_err(|e| {
            LlmError::ProcessError(format!("failed to spawn '{}': {e}", config.command))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LlmError::ProcessError("failed to capture agent stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LlmError::ProcessError("failed to capture agent stdout".to_owned()))?;

        let transport = JsonRpcTransport::new(stdin, stdout);

        // Initialize the ACP connection.
        let init_params = serde_json::json!({
            "protocolVersion": "2025-03-26",
            "clientCapabilities": {},
            "clientInfo": {
                "name": "sober-llm",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let (result, _) = transport.request("initialize", Some(init_params)).await?;
        debug!(result = %result, "ACP initialize response");

        // Extract agent info for model_id.
        if let Some(agent_info) = result.get("agentInfo")
            && let (Some(name), Some(version)) = (
                agent_info.get("name").and_then(|v| v.as_str()),
                agent_info.get("version").and_then(|v| v.as_str()),
            )
        {
            state.model_id_str = format!("acp:{name}/{version}");
        }

        state.child = Some(child);
        state.transport = Some(transport);

        Ok(())
    }

    /// Ensure a session exists, creating one if necessary.
    async fn ensure_session(state: &mut AcpState) -> Result<String, LlmError> {
        if let Some(ref id) = state.session_id {
            return Ok(id.clone());
        }

        let transport = state
            .transport
            .as_ref()
            .ok_or_else(|| LlmError::ProcessError("transport not initialized".to_owned()))?;

        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/tmp".to_owned());

        let params = serde_json::json!({
            "cwd": cwd,
            "mcpServers": []
        });

        let (result, _) = transport.request("session/new", Some(params)).await?;
        debug!(result = %result, "ACP session/new response");

        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                LlmError::InvalidResponse("session/new response missing sessionId".to_owned())
            })?
            .to_owned();

        state.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Convert our messages to ACP prompt content blocks.
    fn messages_to_prompt(messages: &[Message]) -> Vec<serde_json::Value> {
        let text = messages
            .iter()
            .filter_map(|m| {
                m.content.as_ref().map(|c| {
                    if m.role == "system" {
                        format!("[System]: {c}")
                    } else if m.role == "user" {
                        c.clone()
                    } else {
                        format!("[{}]: {c}", m.role)
                    }
                })
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        vec![serde_json::json!({
            "type": "text",
            "text": text
        })]
    }

    /// Parse session update notifications into accumulated text.
    fn collect_notifications(
        notifications: &[JsonRpcNotification],
    ) -> (Option<String>, Option<Usage>) {
        let mut text = String::new();
        let mut usage = None;

        for notif in notifications {
            if let Some(params) = &notif.params
                && let Some(update) = params.get("update")
            {
                let update_type = update
                    .get("sessionUpdate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if update_type == "agent_message_chunk"
                    && let Some(content) = update.get("content")
                    && let Some(t) = content.get("text").and_then(|v| v.as_str())
                {
                    text.push_str(t);
                } else if update_type == "usage_update"
                    && let Some(u) = update.get("usage")
                {
                    usage = serde_json::from_value::<Usage>(u.clone()).ok();
                }
            }
        }

        let content = if text.is_empty() { None } else { Some(text) };
        (content, usage)
    }

    /// Map ACP stop reason to OpenAI finish_reason.
    fn map_stop_reason(stop_reason: &str) -> String {
        match stop_reason {
            "end_turn" => "stop".to_owned(),
            "max_tokens" => "length".to_owned(),
            "refusal" => "stop".to_owned(),
            "cancelled" => "stop".to_owned(),
            other => other.to_owned(),
        }
    }
}

#[async_trait]
impl LlmEngine for AcpEngine {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let mut state = self.state.lock().await;

        Self::ensure_spawned(&mut state, &self.config).await?;
        let session_id = Self::ensure_session(&mut state).await?;

        let provider = "acp";
        let model = state.model_id_str.clone();
        let start = Instant::now();

        let transport = state
            .transport
            .as_ref()
            .ok_or_else(|| LlmError::ProcessError("transport not initialized".to_owned()))?;

        let prompt = Self::messages_to_prompt(&req.messages);
        let params = serde_json::json!({
            "sessionId": session_id,
            "prompt": prompt
        });

        let result = match transport.request("session/prompt", Some(params)).await {
            Ok(r) => r,
            Err(e) => {
                counter!("sober_llm_request_total", "provider" => provider, "model" => model, "status" => "error").increment(1);
                return Err(e);
            }
        };

        let (result, notifications) = result;
        debug!(result = %result, notification_count = notifications.len(), "ACP prompt response");

        let (content, usage) = Self::collect_notifications(&notifications);

        let elapsed = start.elapsed().as_secs_f64();
        counter!("sober_llm_request_total", "provider" => provider, "model" => model.clone(), "status" => "success").increment(1);
        histogram!("sober_llm_request_duration_seconds", "provider" => provider, "model" => model.clone()).record(elapsed);

        if let Some(ref u) = usage {
            counter!("sober_llm_tokens_input_total", "provider" => provider, "model" => model.clone()).increment(u64::from(u.prompt_tokens));
            counter!("sober_llm_tokens_output_total", "provider" => provider, "model" => model)
                .increment(u64::from(u.completion_tokens));
        }

        let stop_reason = result
            .get("stopReason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");

        Ok(CompletionResponse {
            id: format!("acp-{session_id}"),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_owned(),
                    content,
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(Self::map_stop_reason(stop_reason)),
            }],
            usage,
        })
    }

    async fn stream(
        &self,
        req: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError> {
        // For ACP, we collect the full response and emit it as stream chunks.
        // True streaming would require a different transport architecture.
        let response = self.complete(req).await?;

        let chunks: Vec<Result<StreamChunk, LlmError>> = response
            .choices
            .into_iter()
            .map(|choice| {
                Ok(StreamChunk {
                    id: response.id.clone(),
                    choices: vec![StreamChoice {
                        index: choice.index,
                        delta: MessageDelta {
                            role: Some(choice.message.role),
                            content: choice.message.content,
                            tool_calls: None,
                        },
                        finish_reason: choice.finish_reason,
                    }],
                    usage: response.usage,
                })
            })
            .collect();

        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
        Err(LlmError::Unsupported(
            "ACP agents do not support embedding generation".to_owned(),
        ))
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            supports_tools: true,
            supports_streaming: false, // ACP streaming is simulated
            supports_embeddings: false,
            max_context_tokens: 200_000,
        }
    }

    fn model_id(&self) -> &str {
        // Return the config-based default. After initialization, the actual
        // model_id is updated in state. Use model_id_async() for the
        // up-to-date value.
        // This sync accessor returns the initial value to satisfy the trait.
        // The state.model_id_str may be updated after initialization, but
        // we can't block here.
        //
        // Callers that need the updated ID after spawn should use
        // `model_id_async()`.
        //
        // SAFETY: We leak a small string to get a 'static reference.
        // This is acceptable for a long-lived engine instance.
        Box::leak(format!("acp:{}", self.config.name).into_boxed_str())
    }
}

impl Drop for AcpEngine {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.state.get_mut().child {
            let _ = child.start_kill();
            info!("killed ACP agent subprocess");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_to_prompt_basic() {
        let messages = vec![
            Message::system("You are helpful."),
            Message::user("What is 2+2?"),
        ];

        let prompt = AcpEngine::messages_to_prompt(&messages);
        assert_eq!(prompt.len(), 1);
        let text = prompt[0]["text"].as_str().unwrap();
        assert!(text.contains("[System]: You are helpful."));
        assert!(text.contains("What is 2+2?"));
    }

    #[test]
    fn map_stop_reason_variants() {
        assert_eq!(AcpEngine::map_stop_reason("end_turn"), "stop");
        assert_eq!(AcpEngine::map_stop_reason("max_tokens"), "length");
        assert_eq!(AcpEngine::map_stop_reason("refusal"), "stop");
        assert_eq!(AcpEngine::map_stop_reason("cancelled"), "stop");
        assert_eq!(AcpEngine::map_stop_reason("unknown"), "unknown");
    }

    #[test]
    fn collect_notifications_text() {
        let notifications = vec![
            JsonRpcNotification {
                jsonrpc: "2.0".to_owned(),
                method: "sessionUpdate".to_owned(),
                params: Some(serde_json::json!({
                    "sessionId": "s1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "Hello "}
                    }
                })),
            },
            JsonRpcNotification {
                jsonrpc: "2.0".to_owned(),
                method: "sessionUpdate".to_owned(),
                params: Some(serde_json::json!({
                    "sessionId": "s1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "world!"}
                    }
                })),
            },
        ];

        let (content, _usage) = AcpEngine::collect_notifications(&notifications);
        assert_eq!(content.as_deref(), Some("Hello world!"));
    }

    #[test]
    fn collect_notifications_empty() {
        let (content, usage) = AcpEngine::collect_notifications(&[]);
        assert!(content.is_none());
        assert!(usage.is_none());
    }

    #[test]
    fn acp_config_creation() {
        let config = AcpConfig {
            name: "claude-code".to_owned(),
            command: "claude".to_owned(),
            args: vec!["acp".to_owned()],
            env: HashMap::new(),
        };
        let engine = AcpEngine::new(config);
        assert_eq!(engine.model_id(), "acp:claude-code");
    }
}

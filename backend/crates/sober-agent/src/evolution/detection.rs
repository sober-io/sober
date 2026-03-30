//! Self-evolution pattern detection.
//!
//! Gathers recent conversation data, builds a structured prompt, and calls the
//! LLM with propose_* tool definitions to detect evolution opportunities.

use std::sync::Arc;

use sober_core::types::AgentRepos;
use sober_core::types::repo::{ConversationRepo, MessageRepo};
use sober_llm::Message as LlmMessage;
use sober_llm::types::{CompletionRequest, FunctionDefinition, ToolDefinition};
use tracing::{info, warn};

use sober_core::config::EvolutionConfig;

use crate::agent::Agent;

/// The system prompt for the self-evolution detection LLM call.
const DETECTION_SYSTEM_PROMPT: &str = "\
You are the self-evolution engine for Sõber, an AI agent system. Your job is to \
analyse recent conversation patterns and propose improvements.

You have access to four proposal tools:
- propose_tool: Propose a new WASM plugin tool when users repeatedly need a capability that doesn't exist.
- propose_skill: Propose a new prompt-based skill when users frequently request a specific type of assistance.
- propose_instruction: Propose an instruction overlay change when the agent's behavior should be adjusted.
- propose_automation: Propose a scheduled job when users have recurring needs at predictable intervals.

Rules:
1. Only propose evolutions backed by clear patterns in the data — do not speculate.
2. Do not duplicate existing active evolutions (listed below).
3. Limit to at most 5 proposals per cycle.
4. Each proposal must include a confidence score (0.0–1.0) and source_count.
5. If no patterns warrant a proposal, respond with a brief summary and make no tool calls.
";

/// Gathers a compact summary of recent conversation activity for the LLM.
pub(crate) async fn gather_conversation_summary<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    config: &EvolutionConfig,
) -> String {
    let recent_convs = match agent
        .repos()
        .conversations()
        .list_recent(config.detection_conv_limit)
        .await
    {
        Ok(convs) => convs,
        Err(e) => {
            warn!(error = %e, "failed to query recent conversations for detection");
            return "No conversation data available.".to_owned();
        }
    };

    if recent_convs.is_empty() {
        return "No recent conversations.".to_owned();
    }

    let mut lines = Vec::with_capacity(recent_convs.len());

    for conv in &recent_convs {
        let messages = match agent
            .repos()
            .messages()
            .list_by_conversation(conv.id, config.detection_msg_limit)
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => continue,
        };

        let msg_count = messages.len();
        let user_msgs = messages
            .iter()
            .filter(|m| m.role == sober_core::types::enums::MessageRole::User)
            .count();
        let tool_calls: Vec<&str> = messages
            .iter()
            .filter_map(|m| m.metadata.as_ref())
            .filter_map(|meta| meta.get("tool_calls"))
            .filter_map(|v| v.as_array())
            .flat_map(|arr| arr.iter())
            .filter_map(|tc| tc.get("name"))
            .filter_map(|v| v.as_str())
            .collect();

        let title = conv.title.as_deref().unwrap_or("untitled");
        let updated = conv.updated_at.format("%Y-%m-%d %H:%M");

        if tool_calls.is_empty() {
            lines.push(format!(
                "- {title} (user: {}, msgs: {msg_count}, user_msgs: {user_msgs}, updated: {updated})",
                conv.user_id
            ));
        } else {
            let mut tool_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for name in &tool_calls {
                *tool_counts.entry(name).or_default() += 1;
            }
            let tools_str: String = tool_counts
                .iter()
                .map(|(name, count)| format!("{name}×{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "- {title} (user: {}, msgs: {msg_count}, tools: [{tools_str}], updated: {updated})",
                conv.user_id
            ));
        }
    }

    lines.join("\n")
}

/// Runs the LLM detection call with conversation and evolution context.
///
/// Builds a prompt from gathered data, calls the LLM with propose_* tool
/// definitions, then dispatches any returned tool calls to the propose_*
/// tool implementations.
pub(crate) async fn run_detection_llm<R: AgentRepos>(
    agent: &Arc<Agent<R>>,
    config: &EvolutionConfig,
    conversation_summary: &str,
    active_context: &str,
    task_id: &str,
) {
    let user_message = format!(
        "Analyse the following recent conversation activity and active evolutions. \
         Propose improvements if patterns warrant them.\n\n\
         ## Recent conversation activity\n\n{conversation_summary}\n\n\
         ## Active evolutions\n\n{active_context}"
    );

    let propose_tools = build_propose_tool_definitions(agent);

    let model = agent
        .llm_config()
        .as_ref()
        .map(|c| c.model.clone())
        .unwrap_or_else(|| "default".to_owned());

    let req = CompletionRequest {
        model,
        messages: vec![
            LlmMessage::system(DETECTION_SYSTEM_PROMPT),
            LlmMessage::user(&user_message),
        ],
        tools: propose_tools,
        max_tokens: Some(config.detection_max_tokens),
        temperature: Some(config.detection_temperature),
        stop: vec![],
        stream: false,
    };

    let response = match agent.llm().complete(req).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(task_id = %task_id, error = %e, "detection LLM call failed");
            return;
        }
    };

    let tool_calls = response
        .choices
        .first()
        .and_then(|c| c.message.tool_calls.as_ref())
        .cloned()
        .unwrap_or_default();

    if tool_calls.is_empty() {
        let text = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("no response");
        info!(
            task_id = %task_id,
            response = %text,
            "detection LLM proposed no evolutions"
        );
        return;
    }

    info!(
        task_id = %task_id,
        count = tool_calls.len(),
        "detection LLM proposed evolutions"
    );

    let tools = agent.tool_bootstrap().build_static_tools();
    for tc in &tool_calls {
        let tool_name = &tc.function.name;
        let input = match serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                warn!(task_id = %task_id, tool = %tool_name, error = %e, "invalid tool call arguments");
                continue;
            }
        };

        let tool = tools.iter().find(|t| t.metadata().name == *tool_name);
        if let Some(tool) = tool {
            match tool.execute(input).await {
                Ok(output) => {
                    info!(
                        task_id = %task_id,
                        tool = %tool_name,
                        output = %output.content,
                        "detection proposal created"
                    );
                }
                Err(e) => {
                    warn!(
                        task_id = %task_id,
                        tool = %tool_name,
                        error = %e,
                        "detection proposal tool call failed"
                    );
                }
            }
        } else {
            warn!(
                task_id = %task_id,
                tool = %tool_name,
                "detection LLM called unknown tool"
            );
        }
    }
}

/// Builds LLM tool definitions for the propose_* tools.
fn build_propose_tool_definitions<R: AgentRepos>(agent: &Arc<Agent<R>>) -> Vec<ToolDefinition> {
    let all_tools = agent.tool_bootstrap().build_static_tools();
    all_tools
        .iter()
        .filter(|t| t.metadata().name.starts_with("propose_"))
        .map(|t| {
            let meta = t.metadata();
            ToolDefinition {
                r#type: "function".to_owned(),
                function: FunctionDefinition {
                    name: meta.name,
                    description: meta.description,
                    parameters: meta.input_schema,
                },
            }
        })
        .collect()
}

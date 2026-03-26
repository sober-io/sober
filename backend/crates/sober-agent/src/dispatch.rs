//! Tool call dispatch with write-ahead persistence.
//!
//! Extracted from `agent.rs` — the [`execute_tool_calls`] function handles the
//! full lifecycle of each tool call from an LLM response:
//!
//! 1. **Write-ahead** — persist a pending row via [`ToolExecutionRepo`]
//! 2. **Execute** — run the tool (with panic catching)
//! 3. **Record** — update the row with output/error and final status
//!
//! Each status transition emits a [`AgentEvent::ToolExecutionUpdate`] event
//! through both the per-request event channel and the broadcast channel, so
//! clients see real-time progress.

use std::sync::Arc;
use std::time::Instant;

use sober_core::types::AgentRepos;
use sober_core::types::enums::{ToolExecutionSource, ToolExecutionStatus};
use sober_core::types::ids::{ConversationId, MessageId, UserId, WorkspaceId};
use sober_core::types::repo::ToolExecutionRepo;
use sober_core::types::tool::ToolError;
use sober_core::types::tool_execution::CreateToolExecution;
use sober_llm::types::ToolCall as LlmToolCall;
use tokio::sync::mpsc;
use tracing::warn;

use crate::broadcast::ConversationUpdateSender;
use crate::context::AgentContext;
use crate::error::AgentError;
use crate::grpc::proto;
use crate::stream::AgentEvent;
use crate::tools::ToolRegistry;

/// Timeout in seconds for user confirmation of dangerous commands.
const CONFIRM_TIMEOUT_SECS: u64 = 300;

/// The result of a single tool execution, ready to feed back to the LLM.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// LLM-assigned tool call ID (used to match result to call).
    pub tool_call_id: String,
    /// Name of the tool that was executed.
    pub name: String,
    /// Output content from the tool.
    pub output: String,
    /// Whether the output represents an error condition.
    pub is_error: bool,
}

/// Aggregated outcome from dispatching all tool calls in a turn.
#[derive(Debug)]
pub struct DispatchOutcome {
    /// Per-tool results for feeding back to the LLM.
    pub results: Vec<ToolResult>,
    /// Whether any tool that was run was marked `context_modifying`.
    pub any_context_modifying: bool,
    /// Whether any tool call resulted in an error.
    pub any_errors: bool,
}

/// Per-dispatch request data for [`execute_tool_calls`].
pub struct DispatchRequest<'a> {
    /// Tool calls from the LLM response to dispatch.
    pub tool_calls: &'a [LlmToolCall],
    /// The assistant message ID (for FK on tool execution rows).
    pub assistant_message_id: MessageId,
    /// The conversation this dispatch belongs to.
    pub conversation_id: ConversationId,
    /// Per-turn tool registry.
    pub tool_registry: &'a ToolRegistry,
    /// Channel for streaming events back to the caller.
    pub event_tx: &'a mpsc::Sender<Result<AgentEvent, AgentError>>,
    /// The authenticated user.
    pub user_id: UserId,
    /// The workspace associated with this conversation, if any.
    pub workspace_id: Option<WorkspaceId>,
}

/// Details of a confirmation request extracted from [`ToolError::NeedsConfirmation`].
struct ConfirmDetail {
    confirm_id: String,
    command: String,
    risk_level: String,
    reason: String,
}

/// Dispatches a batch of tool calls from an LLM response.
///
/// For each tool call this function:
/// 1. Creates a pending `ToolExecution` row (write-ahead)
/// 2. Sends a `ToolExecutionUpdate` event with status=pending
/// 3. Transitions to running and sends another event
/// 4. Runs the tool (with panic catching via `tokio::spawn`)
/// 5. Transitions to completed/failed and sends a final event
/// 6. Persists the tool result message for audit trail
///
/// Returns a [`DispatchOutcome`] with per-tool results for the next LLM
/// iteration, plus aggregate flags.
pub async fn execute_tool_calls<R: AgentRepos>(
    ctx: &AgentContext<R>,
    req: &DispatchRequest<'_>,
) -> DispatchOutcome {
    let conv_id_str = req.conversation_id.to_string();
    let mut results = Vec::with_capacity(req.tool_calls.len());
    let mut any_context_modifying = false;
    let mut any_errors = false;

    for tc in req.tool_calls {
        let tool_name = &tc.function.name;
        let mut tool_input: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    tool = %tool_name,
                    error = %e,
                    "failed to parse tool call arguments from LLM, using null"
                );
                serde_json::Value::Null
            }
        };

        // Inject caller context into tool input so tools can authorize
        // and associate results with the originating conversation.
        if let serde_json::Value::Object(ref mut map) = tool_input {
            map.entry("owner_id")
                .or_insert_with(|| serde_json::Value::String(req.user_id.to_string()));
            map.entry("conversation_id")
                .or_insert_with(|| serde_json::Value::String(req.conversation_id.to_string()));
            if let Some(ws_id) = req.workspace_id {
                map.entry("workspace_id")
                    .or_insert_with(|| serde_json::Value::String(ws_id.to_string()));
            }
            // TODO: resolve from RoleRepo when RBAC is wired into the agent
            map.entry("is_admin")
                .or_insert(serde_json::Value::Bool(false));
        }

        let _tool_internal = req
            .tool_registry
            .get_tool(tool_name)
            .is_some_and(|t| t.metadata().internal);

        // -----------------------------------------------------------
        // Step 1: Write-ahead — create pending tool execution row
        // -----------------------------------------------------------
        let exec = match ctx
            .repos
            .tool_executions()
            .create_pending(CreateToolExecution {
                conversation_id: req.conversation_id,
                conversation_message_id: req.assistant_message_id,
                tool_call_id: tc.id.clone(),
                tool_name: tool_name.clone(),
                input: tool_input.clone(),
                source: ToolExecutionSource::Builtin,
                plugin_id: None,
            })
            .await
        {
            Ok(exec) => exec,
            Err(e) => {
                warn!(
                    tool = %tool_name,
                    error = %e,
                    "failed to create pending tool execution row"
                );
                // Fall through without write-ahead — push a synthetic error
                // result so the LLM knows persistence failed.
                any_errors = true;
                results.push(ToolResult {
                    tool_call_id: tc.id.clone(),
                    name: tool_name.clone(),
                    output: format!("Internal error: failed to record tool execution: {e}"),
                    is_error: true,
                });
                continue;
            }
        };

        let exec_id = exec.id;
        let exec_id_str = exec_id.to_string();
        let msg_id_str = req.assistant_message_id.to_string();

        // -----------------------------------------------------------
        // Step 2: Send pending event (include input so the frontend can display it)
        // -----------------------------------------------------------
        let input_json = serde_json::to_string(&tool_input).ok();
        send_execution_update(
            req.event_tx,
            &ctx.broadcast_tx,
            &conv_id_str,
            &exec_id_str,
            &msg_id_str,
            &tc.id,
            tool_name,
            ToolExecutionStatus::Pending,
            None,
            None,
            input_json.as_deref(),
        )
        .await;

        // -----------------------------------------------------------
        // Step 3: Transition to running
        // -----------------------------------------------------------
        if let Err(e) = ctx
            .repos
            .tool_executions()
            .update_status(exec_id, ToolExecutionStatus::Running, None, None)
            .await
        {
            warn!(tool = %tool_name, error = %e, "failed to mark tool execution as running");
        }

        send_execution_update(
            req.event_tx,
            &ctx.broadcast_tx,
            &conv_id_str,
            &exec_id_str,
            &msg_id_str,
            &tc.id,
            tool_name,
            ToolExecutionStatus::Running,
            None,
            None,
            None,
        )
        .await;

        // Save the command for audit logging before tool_input is potentially moved.
        let shell_command = if tool_name == "shell" {
            tool_input
                .get("command")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            None
        };

        // -----------------------------------------------------------
        // Step 4: Execute the tool (with panic catch)
        // -----------------------------------------------------------
        let tool_start = Instant::now();
        // Check context_modifying before execution.
        if req
            .tool_registry
            .get_tool(tool_name)
            .is_some_and(|t| t.metadata().context_modifying)
        {
            any_context_modifying = true;
        }

        let (output, is_error) = execute_single_tool(
            ctx,
            req.tool_registry,
            tool_name,
            tool_input.clone(),
            req.event_tx,
            &conv_id_str,
            req.user_id,
        )
        .await;

        if is_error {
            any_errors = true;
        }

        metrics::histogram!(
            "sober_agent_tool_call_duration_seconds",
            "tool" => tool_name.clone(),
        )
        .record(tool_start.elapsed().as_secs_f64());

        // -----------------------------------------------------------
        // Step 5: Transition to completed/failed
        // -----------------------------------------------------------
        let final_status = if is_error {
            ToolExecutionStatus::Failed
        } else {
            ToolExecutionStatus::Completed
        };
        let (db_output, db_error) = if is_error {
            (None, Some(output.as_str()))
        } else {
            (Some(output.as_str()), None)
        };

        if let Err(e) = ctx
            .repos
            .tool_executions()
            .update_status(exec_id, final_status, db_output, db_error)
            .await
        {
            warn!(tool = %tool_name, error = %e, "failed to record tool execution result");
        }

        send_execution_update(
            req.event_tx,
            &ctx.broadcast_tx,
            &conv_id_str,
            &exec_id_str,
            &msg_id_str,
            &tc.id,
            tool_name,
            final_status,
            if !is_error { Some(&output) } else { None },
            if is_error { Some(&output) } else { None },
            None,
        )
        .await;

        // Tool results are now persisted via ToolExecution records (Step 4/5 above).
        // No separate Tool-role message is needed.

        // Audit logging for shell tool runs.
        if let Some(ref cmd) = shell_command {
            let _ = crate::audit::log_shell_exec(
                ctx.repos.audit_log(),
                req.user_id,
                req.workspace_id,
                serde_json::json!({
                    "command": cmd,
                    "conversation_id": req.conversation_id.to_string(),
                }),
            )
            .await;
        }

        results.push(ToolResult {
            tool_call_id: tc.id.clone(),
            name: tool_name.clone(),
            output,
            is_error,
        });
    }

    DispatchOutcome {
        results,
        any_context_modifying,
        any_errors,
    }
}

/// Executes a single tool, handling the confirmation flow and panic catching.
///
/// Returns `(output_string, is_error)`.
async fn execute_single_tool<R: AgentRepos>(
    ctx: &AgentContext<R>,
    tool_registry: &ToolRegistry,
    tool_name: &str,
    tool_input: serde_json::Value,
    event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
    conv_id_str: &str,
    user_id: UserId,
) -> (String, bool) {
    match tool_registry.get_tool(tool_name) {
        Some(tool) => {
            // Wrap execution in tokio::spawn to catch panics — a panicking
            // tool must not tear down the entire agent task.
            let execute_result = {
                let tool_ref = Arc::clone(&tool);
                let input_clone = tool_input.clone();
                let handle = tokio::spawn(async move { tool_ref.execute(input_clone).await });
                match handle.await {
                    Ok(result) => result,
                    Err(join_err) => {
                        // The task panicked or was cancelled.
                        Err(ToolError::ExecutionFailed(format!(
                            "Tool panicked: {join_err}"
                        )))
                    }
                }
            };

            match execute_result {
                Ok(output) => {
                    metrics::counter!(
                        "sober_agent_tool_calls_total",
                        "tool" => tool_name.to_owned(),
                        "status" => "success",
                    )
                    .increment(1);
                    (output.content, output.is_error)
                }
                Err(ToolError::NeedsConfirmation {
                    confirm_id,
                    command,
                    risk_level,
                    reason,
                }) => {
                    let detail = ConfirmDetail {
                        confirm_id,
                        command,
                        risk_level,
                        reason,
                    };
                    metrics::counter!(
                        "sober_agent_tool_calls_total",
                        "tool" => tool_name.to_owned(),
                        "status" => "success",
                    )
                    .increment(1);
                    let output = handle_confirmation(
                        ctx,
                        &tool,
                        tool_input,
                        detail,
                        event_tx,
                        conv_id_str,
                        user_id,
                    )
                    .await;
                    (output, false)
                }
                Err(e) => {
                    metrics::counter!(
                        "sober_agent_tool_calls_total",
                        "tool" => tool_name.to_owned(),
                        "status" => "error",
                    )
                    .increment(1);
                    (format!("Tool error: {e}"), true)
                }
            }
        }
        None => {
            metrics::counter!(
                "sober_agent_tool_calls_total",
                "tool" => tool_name.to_owned(),
                "status" => "not_found",
            )
            .increment(1);
            (format!("Tool not found: {tool_name}"), true)
        }
    }
}

/// Handles the confirmation flow for a tool that returned `NeedsConfirmation`.
///
/// Sends the confirmation event to the client, waits for the user's response,
/// and re-executes the tool if approved. If the tool execution is denied or
/// times out, an appropriate message is returned.
async fn handle_confirmation<R: AgentRepos>(
    ctx: &AgentContext<R>,
    tool: &Arc<dyn sober_core::types::tool::Tool>,
    mut tool_input: serde_json::Value,
    confirm: ConfirmDetail,
    event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
    conv_id_str: &str,
    user_id: UserId,
) -> String {
    let Some(ref registrar) = ctx.registrar else {
        return format!(
            "Command requires confirmation but no confirmation handler is available. \
             Risk level: {}",
            confirm.risk_level
        );
    };

    // Register before sending the event so the broker is ready.
    let rx = registrar.register(confirm.confirm_id.clone());

    let _ = event_tx
        .send(Ok(AgentEvent::ConfirmRequest {
            confirm_id: confirm.confirm_id.clone(),
            command: confirm.command.clone(),
            risk_level: confirm.risk_level.clone(),
            affects: Vec::new(),
            reason: confirm.reason.clone(),
        }))
        .await;
    let _ = ctx.broadcast_tx.send(proto::ConversationUpdate {
        conversation_id: conv_id_str.to_owned(),
        event: Some(proto::conversation_update::Event::ConfirmRequest(
            proto::ConfirmRequest {
                confirm_id: confirm.confirm_id,
                command: confirm.command.clone(),
                risk_level: confirm.risk_level.clone(),
                affects: Vec::new(),
                reason: confirm.reason.clone(),
            },
        )),
    });

    match tokio::time::timeout(std::time::Duration::from_secs(CONFIRM_TIMEOUT_SECS), rx).await {
        Ok(Ok(approved)) => {
            // Log the confirmation decision.
            let _ = crate::audit::log_confirmation(
                ctx.repos.audit_log(),
                user_id,
                approved,
                serde_json::json!({
                    "command": confirm.command,
                    "risk_level": confirm.risk_level,
                    "conversation_id": conv_id_str,
                }),
            )
            .await;

            if approved {
                // Re-execute with confirmed flag.
                if let Some(obj) = tool_input.as_object_mut() {
                    obj.insert("confirmed".to_string(), serde_json::Value::Bool(true));
                }
                match tool.execute(tool_input).await {
                    Ok(output) => output.content,
                    Err(e) => format!("Tool error after confirmation: {e}"),
                }
            } else {
                "Command denied by user.".to_string()
            }
        }
        Ok(Err(_)) => "Confirmation cancelled.".to_string(),
        Err(_) => "Command timed out waiting for confirmation.".to_string(),
    }
}

/// Returns the lowercase string representation of a [`ToolExecutionStatus`].
fn status_to_str(status: ToolExecutionStatus) -> &'static str {
    match status {
        ToolExecutionStatus::Pending => "pending",
        ToolExecutionStatus::Running => "running",
        ToolExecutionStatus::Completed => "completed",
        ToolExecutionStatus::Failed => "failed",
        ToolExecutionStatus::Cancelled => "cancelled",
    }
}

/// Sends a [`AgentEvent::ToolExecutionUpdate`] through both the per-request
/// event channel and the broadcast channel.
#[allow(clippy::too_many_arguments)]
async fn send_execution_update(
    event_tx: &mpsc::Sender<Result<AgentEvent, AgentError>>,
    broadcast_tx: &ConversationUpdateSender,
    conv_id_str: &str,
    exec_id: &str,
    message_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    status: ToolExecutionStatus,
    output: Option<&str>,
    error: Option<&str>,
    input: Option<&str>,
) {
    let status_str = status_to_str(status);

    let _ = event_tx
        .send(Ok(AgentEvent::ToolExecutionUpdate {
            id: exec_id.to_owned(),
            message_id: message_id.to_owned(),
            tool_call_id: tool_call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            status: status_str.to_owned(),
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            input: input.map(str::to_owned),
        }))
        .await;

    let _ = broadcast_tx.send(proto::ConversationUpdate {
        conversation_id: conv_id_str.to_owned(),
        event: Some(proto::conversation_update::Event::ToolExecutionUpdate(
            proto::ToolExecutionUpdate {
                id: exec_id.to_owned(),
                message_id: message_id.to_owned(),
                tool_call_id: tool_call_id.to_owned(),
                tool_name: tool_name.to_owned(),
                status: status_str.to_owned(),
                output: output.map(str::to_owned),
                error: error.map(str::to_owned),
                input: input.map(str::to_owned),
            },
        )),
    });
}

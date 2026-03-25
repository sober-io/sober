//! Conversation history conversion from domain types to LLM message format.
//!
//! The [`to_llm_messages`] function converts a slice of [`MessageWithExecutions`]
//! into a `Vec<LlmMessage>` suitable for sending to an LLM provider. Each domain
//! message is mapped by role, and assistant messages with tool executions produce
//! both the assistant tool-call message and the corresponding tool result messages.

use sober_core::types::enums::{MessageRole, ToolExecutionStatus};
use sober_core::types::tool_execution::{MessageWithExecutions, ToolExecution};
use sober_llm::Message as LlmMessage;
use sober_llm::types::{FunctionCall, ToolCall as LlmToolCall};

/// Converts domain messages (with associated tool executions) into the LLM
/// message format expected by chat completion APIs.
///
/// # Conversion rules
///
/// - `User` messages become `LlmMessage::user(content)`.
/// - `System` messages become `LlmMessage::system(content)`.
/// - `Event` messages are skipped (not relevant to LLM context).
/// - `Assistant` messages with no tool executions and empty content are skipped.
/// - `Assistant` messages with tool executions emit:
///   1. An assistant message with `tool_calls` and optional `reasoning_content`.
///   2. One tool result message per execution, with content based on status.
/// - `Assistant` messages without tool executions emit a plain assistant message.
pub fn to_llm_messages(messages: &[MessageWithExecutions]) -> Vec<LlmMessage> {
    let mut result: Vec<LlmMessage> = Vec::with_capacity(messages.len());

    for mwe in messages {
        let msg = &mwe.message;

        match msg.role {
            MessageRole::Event => continue,

            MessageRole::User => {
                result.push(LlmMessage::user(&msg.content));
            }

            MessageRole::System => {
                result.push(LlmMessage::system(&msg.content));
            }

            MessageRole::Assistant => {
                if mwe.tool_executions.is_empty() {
                    // Skip empty assistant messages (no content, no tool calls).
                    if msg.content.is_empty() {
                        continue;
                    }
                    result.push(LlmMessage {
                        role: "assistant".to_owned(),
                        content: Some(msg.content.clone()),
                        // Thinking-enabled models require reasoning_content on
                        // every assistant message. Use empty string when absent.
                        reasoning_content: Some(msg.reasoning.clone().unwrap_or_default()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                } else {
                    // Build tool_calls from executions.
                    let tool_calls: Vec<LlmToolCall> = mwe
                        .tool_executions
                        .iter()
                        .map(execution_to_tool_call)
                        .collect();

                    // Emit assistant message with tool_calls.
                    result.push(LlmMessage {
                        role: "assistant".to_owned(),
                        content: if msg.content.is_empty() {
                            None
                        } else {
                            Some(msg.content.clone())
                        },
                        // Thinking-enabled models require reasoning_content on
                        // every assistant message. Use empty string when absent.
                        reasoning_content: Some(msg.reasoning.clone().unwrap_or_default()),
                        tool_calls: Some(tool_calls),
                        tool_call_id: None,
                    });

                    // Emit tool result messages.
                    for exec in &mwe.tool_executions {
                        let content = tool_result_content(exec);
                        result.push(LlmMessage::tool(&exec.tool_call_id, content));
                    }
                }
            }
        }
    }

    result
}

/// Converts a [`ToolExecution`] into an LLM [`ToolCall`](LlmToolCall).
fn execution_to_tool_call(exec: &ToolExecution) -> LlmToolCall {
    LlmToolCall {
        id: exec.tool_call_id.clone(),
        r#type: "function".to_owned(),
        function: FunctionCall {
            name: exec.tool_name.clone(),
            arguments: exec.input.to_string(),
        },
    }
}

/// Determines the tool result content string based on execution status.
fn tool_result_content(exec: &ToolExecution) -> String {
    match exec.status {
        ToolExecutionStatus::Completed => exec.output.clone().unwrap_or_default(),
        ToolExecutionStatus::Failed => {
            let error = exec.error.as_deref().unwrap_or("Unknown error");
            format!("Error: {error}")
        }
        ToolExecutionStatus::Pending | ToolExecutionStatus::Running => {
            "Error: Agent restarted during execution".to_owned()
        }
        ToolExecutionStatus::Cancelled => "Cancelled by user".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use sober_core::types::domain::Message;
    use sober_core::types::enums::ToolExecutionSource;
    use sober_core::types::ids::{ConversationId, MessageId, ToolExecutionId};

    /// Helper: build a domain message with a given role and content.
    fn make_message(role: MessageRole, content: &str) -> Message {
        Message {
            id: MessageId::new(),
            conversation_id: ConversationId::new(),
            role,
            content: content.to_owned(),
            reasoning: None,
            token_count: None,
            user_id: None,
            metadata: None,
            created_at: Utc::now(),
        }
    }

    /// Helper: build a `MessageWithExecutions` with no tool executions.
    fn mwe(role: MessageRole, content: &str) -> MessageWithExecutions {
        MessageWithExecutions {
            message: make_message(role, content),
            tool_executions: vec![],
        }
    }

    /// Helper: build a tool execution with the given status, output, and error.
    fn make_execution(
        message_id: MessageId,
        conversation_id: ConversationId,
        tool_call_id: &str,
        tool_name: &str,
        status: ToolExecutionStatus,
        output: Option<&str>,
        error: Option<&str>,
    ) -> ToolExecution {
        ToolExecution {
            id: ToolExecutionId::new(),
            conversation_id,
            conversation_message_id: message_id,
            tool_call_id: tool_call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: serde_json::json!({"query": "test"}),
            source: ToolExecutionSource::Builtin,
            status,
            output: output.map(|s| s.to_owned()),
            error: error.map(|s| s.to_owned()),
            plugin_id: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let result = to_llm_messages(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn user_message_converts() {
        let messages = vec![mwe(MessageRole::User, "Hello!")];
        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].content.as_deref(), Some("Hello!"));
    }

    #[test]
    fn system_message_converts() {
        let messages = vec![mwe(MessageRole::System, "You are helpful.")];
        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn event_messages_are_skipped() {
        let messages = vec![
            mwe(MessageRole::User, "Hello"),
            mwe(MessageRole::Event, "user_joined"),
            mwe(MessageRole::Assistant, "Hi there"),
        ];
        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
    }

    #[test]
    fn empty_assistant_without_tools_is_skipped() {
        let messages = vec![
            mwe(MessageRole::User, "Hello"),
            mwe(MessageRole::Assistant, ""),
        ];
        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn assistant_without_tools_preserves_content() {
        let messages = vec![mwe(MessageRole::Assistant, "Here is the answer.")];
        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "assistant");
        assert_eq!(result[0].content.as_deref(), Some("Here is the answer."));
        assert!(result[0].tool_calls.is_none());
    }

    #[test]
    fn assistant_without_tools_preserves_reasoning() {
        let mut m = mwe(MessageRole::Assistant, "Answer");
        m.message.reasoning = Some("I thought about it.".to_owned());
        let result = to_llm_messages(&[m]);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].reasoning_content.as_deref(),
            Some("I thought about it.")
        );
    }

    #[test]
    fn assistant_with_completed_tool_execution() {
        let msg = make_message(MessageRole::Assistant, "Let me search.");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_abc",
            "web_search",
            ToolExecutionStatus::Completed,
            Some("Search results here"),
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);

        // Should produce: assistant message + tool result message
        assert_eq!(result.len(), 2);

        // Assistant message with tool_calls
        assert_eq!(result[0].role, "assistant");
        assert_eq!(result[0].content.as_deref(), Some("Let me search."));
        let tool_calls = result[0]
            .tool_calls
            .as_ref()
            .expect("should have tool_calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc");
        assert_eq!(tool_calls[0].function.name, "web_search");
        assert_eq!(tool_calls[0].r#type, "function");

        // Tool result message
        assert_eq!(result[1].role, "tool");
        assert_eq!(result[1].tool_call_id.as_deref(), Some("call_abc"));
        assert_eq!(result[1].content.as_deref(), Some("Search results here"));
    }

    #[test]
    fn assistant_with_failed_tool_execution() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_fail",
            "broken_tool",
            ToolExecutionStatus::Failed,
            None,
            Some("connection timeout"),
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 2);

        // Assistant message: empty content becomes None
        assert_eq!(result[0].role, "assistant");
        assert!(result[0].content.is_none());

        // Tool result with error
        assert_eq!(result[1].role, "tool");
        assert_eq!(
            result[1].content.as_deref(),
            Some("Error: connection timeout")
        );
    }

    #[test]
    fn failed_tool_execution_with_no_error_message() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_fail2",
            "tool",
            ToolExecutionStatus::Failed,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(result[1].content.as_deref(), Some("Error: Unknown error"));
    }

    #[test]
    fn pending_tool_execution_shows_restart_message() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_pending",
            "slow_tool",
            ToolExecutionStatus::Pending,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(
            result[1].content.as_deref(),
            Some("Error: Agent restarted during execution")
        );
    }

    #[test]
    fn running_tool_execution_shows_restart_message() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_running",
            "long_tool",
            ToolExecutionStatus::Running,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(
            result[1].content.as_deref(),
            Some("Error: Agent restarted during execution")
        );
    }

    #[test]
    fn cancelled_tool_execution() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_cancel",
            "some_tool",
            ToolExecutionStatus::Cancelled,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(result[1].content.as_deref(), Some("Cancelled by user"));
    }

    #[test]
    fn multiple_tool_executions_on_one_assistant_message() {
        let msg = make_message(MessageRole::Assistant, "I'll do both.");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec1 = make_execution(
            msg_id,
            conv_id,
            "call_1",
            "search",
            ToolExecutionStatus::Completed,
            Some("result 1"),
            None,
        );
        let exec2 = make_execution(
            msg_id,
            conv_id,
            "call_2",
            "calculate",
            ToolExecutionStatus::Completed,
            Some("result 2"),
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec1, exec2],
        }];

        let result = to_llm_messages(&messages);

        // assistant + 2 tool results
        assert_eq!(result.len(), 3);

        let tool_calls = result[0]
            .tool_calls
            .as_ref()
            .expect("should have tool_calls");
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[1].id, "call_2");

        assert_eq!(result[1].role, "tool");
        assert_eq!(result[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(result[1].content.as_deref(), Some("result 1"));

        assert_eq!(result[2].role, "tool");
        assert_eq!(result[2].tool_call_id.as_deref(), Some("call_2"));
        assert_eq!(result[2].content.as_deref(), Some("result 2"));
    }

    #[test]
    fn mixed_tool_execution_statuses() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec_completed = make_execution(
            msg_id,
            conv_id,
            "call_ok",
            "tool_a",
            ToolExecutionStatus::Completed,
            Some("success"),
            None,
        );
        let exec_failed = make_execution(
            msg_id,
            conv_id,
            "call_err",
            "tool_b",
            ToolExecutionStatus::Failed,
            None,
            Some("boom"),
        );
        let exec_cancelled = make_execution(
            msg_id,
            conv_id,
            "call_cancel",
            "tool_c",
            ToolExecutionStatus::Cancelled,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec_completed, exec_failed, exec_cancelled],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(result.len(), 4); // 1 assistant + 3 tool results

        assert_eq!(result[1].content.as_deref(), Some("success"));
        assert_eq!(result[2].content.as_deref(), Some("Error: boom"));
        assert_eq!(result[3].content.as_deref(), Some("Cancelled by user"));
    }

    #[test]
    fn full_conversation_flow() {
        let messages = vec![
            mwe(MessageRole::System, "You are helpful."),
            mwe(MessageRole::User, "What's the weather?"),
            {
                let msg = make_message(MessageRole::Assistant, "Let me check.");
                let conv_id = msg.conversation_id;
                let msg_id = msg.id;
                let exec = make_execution(
                    msg_id,
                    conv_id,
                    "call_weather",
                    "get_weather",
                    ToolExecutionStatus::Completed,
                    Some(r#"{"temp": 72, "condition": "sunny"}"#),
                    None,
                );
                MessageWithExecutions {
                    message: msg,
                    tool_executions: vec![exec],
                }
            },
            mwe(MessageRole::Assistant, "It's 72F and sunny!"),
            mwe(MessageRole::Event, "title_changed"),
            mwe(MessageRole::User, "Thanks!"),
        ];

        let result = to_llm_messages(&messages);

        // system + user + assistant(tool) + tool_result + assistant + user = 6
        assert_eq!(result.len(), 6);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[1].role, "user");
        assert_eq!(result[2].role, "assistant");
        assert!(result[2].tool_calls.is_some());
        assert_eq!(result[3].role, "tool");
        assert_eq!(result[4].role, "assistant");
        assert!(result[4].tool_calls.is_none());
        assert_eq!(result[5].role, "user");
    }

    #[test]
    fn tool_call_arguments_serialized_as_json_string() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let mut exec = make_execution(
            msg_id,
            conv_id,
            "call_x",
            "my_tool",
            ToolExecutionStatus::Completed,
            Some("ok"),
            None,
        );
        exec.input = serde_json::json!({"city": "London", "units": "metric"});

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        let tool_calls = result[0].tool_calls.as_ref().unwrap();
        // arguments should be a JSON string representation
        let args: serde_json::Value =
            serde_json::from_str(&tool_calls[0].function.arguments).expect("valid JSON");
        assert_eq!(args["city"], "London");
        assert_eq!(args["units"], "metric");
    }

    #[test]
    fn completed_execution_with_no_output_returns_empty_string() {
        let msg = make_message(MessageRole::Assistant, "");
        let conv_id = msg.conversation_id;
        let msg_id = msg.id;

        let exec = make_execution(
            msg_id,
            conv_id,
            "call_empty",
            "void_tool",
            ToolExecutionStatus::Completed,
            None,
            None,
        );

        let messages = vec![MessageWithExecutions {
            message: msg,
            tool_executions: vec![exec],
        }];

        let result = to_llm_messages(&messages);
        assert_eq!(result[1].content.as_deref(), Some(""));
    }
}

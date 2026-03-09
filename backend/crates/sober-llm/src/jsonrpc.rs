//! JSON-RPC 2.0 transport over stdin/stdout for ACP subprocess communication.
//!
//! The ACP protocol uses newline-delimited JSON-RPC 2.0 messages over the
//! subprocess's stdin (for sending) and stdout (for receiving).

use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

use crate::error::LlmError;

/// A JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: i64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response (success or error).
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcErrorData>,
}

/// JSON-RPC error payload.
#[derive(Debug, Deserialize)]
pub struct JsonRpcErrorData {
    pub code: i64,
    pub message: String,
}

/// A JSON-RPC 2.0 notification (no `id` field).
#[derive(Debug, Deserialize)]
pub struct JsonRpcNotification {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// A raw JSON-RPC message from stdout — could be a response or a notification.
#[derive(Debug)]
pub enum JsonRpcMessage {
    /// A response to a request we sent (has matching `id`).
    Response(JsonRpcResponse),
    /// An unsolicited notification from the agent (no `id`).
    Notification(JsonRpcNotification),
}

/// Bidirectional JSON-RPC 2.0 transport over child process stdin/stdout.
pub struct JsonRpcTransport {
    writer: Mutex<ChildStdin>,
    reader: Mutex<BufReader<ChildStdout>>,
    next_id: AtomicI64,
}

impl JsonRpcTransport {
    /// Create a new transport from child process handles.
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            writer: Mutex::new(stdin),
            reader: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicI64::new(1),
        }
    }

    /// Send a JSON-RPC request and return the assigned request ID.
    pub async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<i64, LlmError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_owned(),
            params,
        };

        let mut json = serde_json::to_string(&request)
            .map_err(|e| LlmError::StreamError(format!("failed to serialize request: {e}")))?;
        json.push('\n');

        trace!(method = %method, id = id, "sending JSON-RPC request");

        let mut writer = self.writer.lock().await;
        writer
            .write_all(json.as_bytes())
            .await
            .map_err(|e| LlmError::ProcessError(format!("failed to write to stdin: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| LlmError::ProcessError(format!("failed to flush stdin: {e}")))?;

        Ok(id)
    }

    /// Send a JSON-RPC notification (no `id`, no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), LlmError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let mut json = serde_json::to_string(&msg)
            .map_err(|e| LlmError::StreamError(format!("failed to serialize notification: {e}")))?;
        json.push('\n');

        trace!(method = %method, "sending JSON-RPC notification");

        let mut writer = self.writer.lock().await;
        writer
            .write_all(json.as_bytes())
            .await
            .map_err(|e| LlmError::ProcessError(format!("failed to write to stdin: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| LlmError::ProcessError(format!("failed to flush stdin: {e}")))?;

        Ok(())
    }

    /// Read the next JSON-RPC message from stdout.
    ///
    /// Returns either a response (has `id`) or a notification (no `id`).
    pub async fn read_message(&self) -> Result<JsonRpcMessage, LlmError> {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| LlmError::ProcessError(format!("failed to read from stdout: {e}")))?;

            if bytes_read == 0 {
                return Err(LlmError::ProcessError(
                    "agent subprocess closed stdout".to_owned(),
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            trace!(raw = %trimmed, "received JSON-RPC message");

            let value: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| LlmError::InvalidResponse(format!("invalid JSON from agent: {e}")))?;

            // Distinguish response (has `id`) from notification (no `id` or `id: null`).
            if value.get("id").is_some_and(|id| !id.is_null()) {
                let response: JsonRpcResponse = serde_json::from_value(value).map_err(|e| {
                    LlmError::InvalidResponse(format!("failed to parse response: {e}"))
                })?;
                return Ok(JsonRpcMessage::Response(response));
            }

            let notification: JsonRpcNotification = serde_json::from_value(value).map_err(|e| {
                LlmError::InvalidResponse(format!("failed to parse notification: {e}"))
            })?;
            return Ok(JsonRpcMessage::Notification(notification));
        }
    }

    /// Send a request and wait for the matching response, collecting
    /// any notifications received in between.
    ///
    /// Returns `(response_result, collected_notifications)`.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(serde_json::Value, Vec<JsonRpcNotification>), LlmError> {
        let req_id = self.send_request(method, params).await?;
        let mut notifications = Vec::new();

        loop {
            let msg = self.read_message().await?;
            match msg {
                JsonRpcMessage::Response(resp) => {
                    // Check if this response matches our request.
                    let resp_id = resp.id.as_ref().and_then(|v| v.as_i64());
                    if resp_id == Some(req_id) {
                        if let Some(err) = resp.error {
                            return Err(LlmError::JsonRpcError {
                                code: err.code,
                                message: err.message,
                            });
                        }
                        let result = resp.result.unwrap_or(serde_json::Value::Null);
                        return Ok((result, notifications));
                    }
                    // Response for a different request — shouldn't happen in our
                    // single-request-at-a-time model, but log and continue.
                    warn!(
                        expected_id = req_id,
                        actual_id = ?resp_id,
                        "received response for unexpected request ID"
                    );
                }
                JsonRpcMessage::Notification(notif) => {
                    debug!(method = %notif.method, "received notification while awaiting response");
                    notifications.push(notif);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_request_serialization() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize".to_owned(),
            params: Some(serde_json::json!({"protocolVersion": "2025-03-26"})),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "initialize");
        assert!(json["params"].is_object());
    }

    #[test]
    fn json_rpc_response_deserialization_success() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"sessionId": "sess-123"}
        }"#;

        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn json_rpc_response_deserialization_error() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32600, "message": "Invalid Request"}
        }"#;

        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn json_rpc_notification_deserialization() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "sessionUpdate",
            "params": {
                "sessionId": "sess-123",
                "update": {"sessionUpdate": "agent_message_chunk"}
            }
        }"#;

        let notif: JsonRpcNotification = serde_json::from_str(json).unwrap();
        assert_eq!(notif.method, "sessionUpdate");
        assert!(notif.params.is_some());
    }

    #[test]
    fn request_without_params() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "shutdown".to_owned(),
            params: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("params"));
    }
}

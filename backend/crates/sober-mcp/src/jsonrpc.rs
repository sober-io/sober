//! JSON-RPC 2.0 message types for MCP communication.
//!
//! These types are crate-internal and not exposed in the public API.

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JsonRpcRequest {
    /// Protocol version (always "2.0").
    pub jsonrpc: String,
    /// Request identifier.
    pub id: u64,
    /// Method name.
    pub method: String,
    /// Method parameters (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request.
    pub fn new(id: u64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JsonRpcNotification {
    /// Protocol version (always "2.0").
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Method parameters (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC notification.
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JsonRpcResponse {
    /// Protocol version (always "2.0").
    pub jsonrpc: String,
    /// Request identifier this response corresponds to.
    pub id: Option<u64>,
    /// Result on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JsonRpcError {
    /// Error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
    /// Additional error data (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_correctly() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            Some(serde_json::json!({"protocolVersion": "2025-03-26"})),
        );
        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "initialize");
        assert_eq!(json["params"]["protocolVersion"], "2025-03-26");
    }

    #[test]
    fn request_without_params_omits_field() {
        let req = JsonRpcRequest::new(2, "tools/list", None);
        let json_str = serde_json::to_string(&req).expect("serialize");
        assert!(!json_str.contains("params"));
    }

    #[test]
    fn notification_serializes_correctly() {
        let notif = JsonRpcNotification::new("notifications/initialized", None);
        let json = serde_json::to_value(&notif).expect("serialize");
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "notifications/initialized");
        assert!(json.get("id").is_none());
    }

    #[test]
    fn response_with_result_deserializes() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"protocolVersion": "2025-03-26", "serverInfo": {"name": "test"}}
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).expect("deserialize");
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_with_error_deserializes() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 2,
            "error": {"code": -32601, "message": "Method not found"}
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).expect("deserialize");
        assert_eq!(resp.id, Some(2));
        assert!(resp.result.is_none());
        let err = resp.error.expect("should have error");
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn response_roundtrip() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_owned(),
            id: Some(3),
            result: Some(serde_json::json!({"tools": []})),
            error: None,
        };
        let json_str = serde_json::to_string(&resp).expect("serialize");
        let deserialized: JsonRpcResponse = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(deserialized.id, Some(3));
        assert!(deserialized.result.is_some());
    }

    #[test]
    fn error_with_data_deserializes() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 4,
            "error": {"code": -32000, "message": "custom", "data": {"detail": "extra"}}
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).expect("deserialize");
        let err = resp.error.expect("should have error");
        assert_eq!(err.code, -32000);
        assert!(err.data.is_some());
    }
}

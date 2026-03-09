//! Integration tests for sober-mcp using a mock MCP server.
//!
//! The mock server is a Python script that speaks JSON-RPC 2.0 over stdio,
//! implementing the MCP 2025-03-26 protocol subset needed for testing.

use std::path::PathBuf;

use sober_mcp::client::McpClient;
use sober_mcp::config::McpConfig;
use tokio::process::Command;

/// Path to the mock MCP server script, relative to the crate root.
fn mock_server_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("mock_mcp_server.py");
    path
}

/// Spawn the mock server as a child process (bypassing sandbox for tests).
async fn spawn_mock_client() -> McpClient {
    let server_path = mock_server_path();

    let child = Command::new("python3")
        .arg(&server_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn mock MCP server");

    let config = McpConfig {
        request_timeout_secs: 10,
        ..McpConfig::default()
    };

    McpClient::from_process(child, config).expect("failed to create client from process")
}

#[tokio::test]
async fn test_initialize() {
    let mut client = spawn_mock_client().await;

    client
        .initialize()
        .await
        .expect("initialize should succeed");

    let info = client.server_info().expect("should have server info");
    assert_eq!(info.name, "mock-mcp-server");
    assert_eq!(info.version.as_deref(), Some("1.0.0"));

    let caps = client.capabilities().expect("should have capabilities");
    assert!(caps.has_tools());
    assert!(caps.has_resources());

    client.shutdown().await;
}

#[tokio::test]
async fn test_list_tools() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let tools = client
        .list_tools()
        .await
        .expect("list_tools should succeed");

    assert_eq!(tools.len(), 2);

    let echo = tools.iter().find(|t| t.name == "echo").expect("echo tool");
    assert_eq!(
        echo.description.as_deref(),
        Some("Echoes back the input text")
    );
    assert!(echo.input_schema.is_object());

    let add = tools.iter().find(|t| t.name == "add").expect("add tool");
    assert_eq!(add.description.as_deref(), Some("Adds two numbers"));

    client.shutdown().await;
}

#[tokio::test]
async fn test_call_tool_echo() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let result = client
        .call_tool("echo", serde_json::json!({"text": "hello world"}))
        .await
        .expect("call_tool should succeed");

    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);

    match &result.content[0] {
        sober_mcp::types::ToolCallContent::Text { text } => {
            assert_eq!(text, "Echo: hello world");
        }
        _ => panic!("expected text content"),
    }

    client.shutdown().await;
}

#[tokio::test]
async fn test_call_tool_add() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let result = client
        .call_tool("add", serde_json::json!({"a": 17, "b": 25}))
        .await
        .expect("call_tool should succeed");

    assert!(!result.is_error);
    match &result.content[0] {
        sober_mcp::types::ToolCallContent::Text { text } => {
            assert_eq!(text, "42");
        }
        _ => panic!("expected text content"),
    }

    client.shutdown().await;
}

#[tokio::test]
async fn test_list_resources() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let resources = client
        .list_resources()
        .await
        .expect("list_resources should succeed");

    assert_eq!(resources.len(), 2);

    let greeting = resources
        .iter()
        .find(|r| r.name == "greeting")
        .expect("greeting resource");
    assert_eq!(greeting.uri, "test://greeting");
    assert_eq!(greeting.mime_type.as_deref(), Some("text/plain"));

    let data = resources
        .iter()
        .find(|r| r.name == "data")
        .expect("data resource");
    assert_eq!(data.uri, "test://data");

    client.shutdown().await;
}

#[tokio::test]
async fn test_read_resource() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let contents = client
        .read_resource("test://greeting")
        .await
        .expect("read_resource should succeed");

    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].uri, "test://greeting");
    assert_eq!(
        contents[0].text.as_deref(),
        Some("Hello from mock MCP server!")
    );

    client.shutdown().await;
}

#[tokio::test]
async fn test_read_resource_json() {
    let mut client = spawn_mock_client().await;
    client.initialize().await.expect("initialize");

    let contents = client
        .read_resource("test://data")
        .await
        .expect("read_resource should succeed");

    assert_eq!(contents.len(), 1);
    let text = contents[0].text.as_deref().expect("should have text");
    let parsed: serde_json::Value = serde_json::from_str(text).expect("should be valid JSON");
    assert_eq!(parsed["key"], "value");
    assert_eq!(parsed["count"], 42);

    client.shutdown().await;
}

#[tokio::test]
async fn test_is_alive() {
    let mut client = spawn_mock_client().await;
    assert!(client.is_alive(), "client should be alive after spawn");

    client.initialize().await.expect("initialize");
    assert!(client.is_alive(), "client should be alive after initialize");

    client.shutdown().await;
    // After shutdown + kill, is_alive should eventually return false.
    // Give it a moment.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !client.is_alive(),
        "client should not be alive after shutdown"
    );
}

#[tokio::test]
async fn test_full_lifecycle() {
    // 1. Connect
    let mut client = spawn_mock_client().await;
    assert!(client.is_alive());

    // 2. Initialize
    client.initialize().await.expect("initialize");
    let info = client.server_info().expect("server info");
    assert_eq!(info.name, "mock-mcp-server");

    // 3. Discover tools
    let tools = client.list_tools().await.expect("list tools");
    assert_eq!(tools.len(), 2);

    // 4. Call a tool
    let result = client
        .call_tool("echo", serde_json::json!({"text": "lifecycle test"}))
        .await
        .expect("call tool");
    assert!(!result.is_error);

    // 5. Discover resources
    let resources = client.list_resources().await.expect("list resources");
    assert_eq!(resources.len(), 2);

    // 6. Read a resource
    let contents = client
        .read_resource("test://greeting")
        .await
        .expect("read resource");
    assert_eq!(contents.len(), 1);

    // 7. Shutdown
    client.shutdown().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!client.is_alive());
}

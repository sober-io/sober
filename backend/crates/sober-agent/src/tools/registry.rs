//! Tool registry for discovering and invoking agent tools.
//!
//! The [`ToolRegistry`] collects built-in and MCP tools behind a uniform
//! lookup interface.  It can produce OpenAI-format [`ToolDefinition`]s for
//! LLM requests and resolve a tool by name for execution.

use std::sync::Arc;

use sober_core::types::tool::{Tool, ToolMetadata};
use sober_llm::types::{FunctionDefinition, ToolDefinition};

/// Registry that holds a set of tools and provides lookup by name.
///
/// Built with [`ToolRegistry::with_builtins`] and optionally extended via
/// [`ToolRegistry::with_additional`], which returns a *new* registry
/// (the original remains unchanged).
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates an empty tool registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Creates a registry pre-populated with built-in tools.
    pub fn with_builtins(builtins: Vec<Arc<dyn Tool>>) -> Self {
        Self { tools: builtins }
    }

    /// Returns a **new** registry that merges the current tools with
    /// `additional` (e.g. MCP-discovered tools).  The original registry
    /// is not modified.
    #[must_use]
    pub fn with_additional(&self, additional: Vec<Arc<dyn Tool>>) -> Self {
        let mut tools = self.tools.clone();
        tools.extend(additional);
        Self { tools }
    }

    /// Returns OpenAI-format tool definitions for all registered tools.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_definitions_except(&[])
    }

    /// Returns tool definitions excluding tools with the given names.
    pub fn tool_definitions_except(&self, exclude: &[&str]) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|t| !exclude.contains(&t.metadata().name.as_str()))
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

    /// Returns [`ToolMetadata`] for every registered tool.
    pub fn tool_metadata(&self) -> Vec<ToolMetadata> {
        self.tools.iter().map(|t| t.metadata()).collect()
    }

    /// Looks up a tool by its unique name.
    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|t| t.metadata().name == name)
            .cloned()
    }

    /// Returns the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns `true` if the registry contains no tools.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use sober_core::types::tool::{BoxToolFuture, ToolOutput};

    use super::*;

    /// A minimal test tool for unit testing.
    struct DummyTool {
        name: &'static str,
    }

    impl Tool for DummyTool {
        fn metadata(&self) -> ToolMetadata {
            ToolMetadata {
                name: self.name.to_owned(),
                description: format!("Dummy tool: {}", self.name),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                context_modifying: false,
            }
        }

        fn execute(&self, _input: serde_json::Value) -> BoxToolFuture<'_> {
            Box::pin(async {
                Ok(ToolOutput {
                    content: "ok".to_owned(),
                    is_error: false,
                })
            })
        }
    }

    #[test]
    fn empty_registry() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get_tool("anything").is_none());
        assert!(reg.tool_definitions().is_empty());
        assert!(reg.tool_metadata().is_empty());
    }

    #[test]
    fn register_builtins() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(DummyTool { name: "alpha" }),
            Arc::new(DummyTool { name: "beta" }),
        ];
        let reg = ToolRegistry::with_builtins(tools);
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());
    }

    #[test]
    fn lookup_by_name() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(DummyTool { name: "alpha" }),
            Arc::new(DummyTool { name: "beta" }),
        ];
        let reg = ToolRegistry::with_builtins(tools);

        let found = reg.get_tool("beta").expect("should find beta");
        assert_eq!(found.metadata().name, "beta");
        assert!(reg.get_tool("gamma").is_none());
    }

    #[test]
    fn with_additional_merges_and_preserves_original() {
        let builtins: Vec<Arc<dyn Tool>> = vec![Arc::new(DummyTool { name: "builtin" })];
        let original = ToolRegistry::with_builtins(builtins);

        let extra: Vec<Arc<dyn Tool>> = vec![Arc::new(DummyTool { name: "mcp_tool" })];
        let merged = original.with_additional(extra);

        // Original unchanged
        assert_eq!(original.len(), 1);
        assert!(original.get_tool("mcp_tool").is_none());

        // Merged has both
        assert_eq!(merged.len(), 2);
        assert!(merged.get_tool("builtin").is_some());
        assert!(merged.get_tool("mcp_tool").is_some());
    }

    #[test]
    fn tool_definitions_format() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(DummyTool { name: "my_tool" })];
        let reg = ToolRegistry::with_builtins(tools);

        let defs = reg.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].r#type, "function");
        assert_eq!(defs[0].function.name, "my_tool");
        assert_eq!(defs[0].function.description, "Dummy tool: my_tool");
    }
}

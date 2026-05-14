pub mod bash;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod list_dir;
pub mod memory;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::future::BoxFuture;
use omnix_protocol::{PermissionMode, ToolDefinition};
use thiserror::Error;

pub trait Tool: Send + Sync {
    /// Unique tool name (kebab-case, no spaces).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's input parameters.
    fn input_schema(&self) -> serde_json::Value;

    /// Minimum permission level required to run this tool.
    fn required_permission(&self) -> PermissionMode;

    /// Execute the tool with the given input and context.
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> BoxFuture<'_, Result<ToolOutput, ToolError>>;
}

/// Execution context passed to every tool call.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub env_vars: HashMap<String, String>,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            working_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout: Duration::from_secs(30),
            env_vars: std::env::vars().collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub model_content: Option<String>,
    pub is_error: bool,
    pub metadata: HashMap<String, serde_json::Value>,
}

const MODEL_CONTENT_MAX_LINES: usize = 2_000;
const MODEL_CONTENT_MAX_BYTES: usize = 50 * 1024;

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            model_content: None,
            is_error: false,
            metadata: HashMap::new(),
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            model_content: None,
            is_error: true,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn with_limited_model_content(mut self) -> Self {
        self.model_content = truncate_for_model(&self.content);
        self
    }
}

fn truncate_for_model(content: &str) -> Option<String> {
    let total_lines = content.lines().count();
    let total_bytes = content.len();

    if total_lines <= MODEL_CONTENT_MAX_LINES && total_bytes <= MODEL_CONTENT_MAX_BYTES {
        return None;
    }

    let mut truncated = String::new();
    let mut kept_lines = 0;

    for line in content.lines() {
        if kept_lines >= MODEL_CONTENT_MAX_LINES {
            break;
        }

        let separator_len = usize::from(!truncated.is_empty());
        if truncated.len() + separator_len + line.len() > MODEL_CONTENT_MAX_BYTES {
            break;
        }

        if !truncated.is_empty() {
            truncated.push('\n');
        }
        truncated.push_str(line);
        kept_lines += 1;
    }

    if truncated.is_empty() && !content.is_empty() {
        let end = content
            .char_indices()
            .map(|(idx, _)| idx)
            .take_while(|idx| *idx <= MODEL_CONTENT_MAX_BYTES)
            .last()
            .unwrap_or(0);
        truncated.push_str(&content[..end]);
    }

    let shown_bytes = truncated.len();
    let shown_lines = truncated.lines().count();
    truncated.push_str(&format!(
        "\n\n... (truncated for model context: {shown_lines} of {total_lines} lines, {shown_bytes} of {total_bytes} bytes shown; full output remains visible in the tool card)"
    ));

    Some(truncated)
}

#[derive(Error, Debug, Clone)]
pub enum ToolError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("tool execution timed out after {0:?}")]
    Timeout(Duration),

    #[error("io error: {0}")]
    Io(String),

    #[error("tool not found: {0}")]
    NotFound(String),

    #[error("other error: {0}")]
    Other(String),
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    disabled: HashSet<String>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            disabled: HashSet::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn Tool>> {
        self.tools.values().collect()
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .filter(|t| !self.disabled.contains(t.name()))
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
                required_permission: tool.required_permission(),
            })
            .collect()
    }

    pub fn enable(&mut self, name: &str) {
        self.disabled.remove(name);
    }

    pub fn disable(&mut self, name: &str) {
        self.disabled.insert(name.to_string());
    }

    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        if self.disabled.contains(name) {
            return Err(ToolError::Other(format!("Tool '{}' is disabled", name)));
        }
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.execute(input, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct DummyTool;

    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }

        fn description(&self) -> &str {
            "A dummy tool for testing"
        }

        fn input_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "value": {"type": "integer"}
                }
            })
        }

        fn required_permission(&self) -> PermissionMode {
            PermissionMode::ReadOnly
        }

        fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &ToolContext,
        ) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
            Box::pin(async move {
                let value = input["value"].as_i64().unwrap_or(0);
                Ok(ToolOutput::ok(format!("got {}", value)))
            })
        }
    }

    #[test]
    fn registry_register_and_list() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        let tools = registry.list();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "dummy");
    }

    #[test]
    fn registry_get_existing() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        let tool = registry.get("dummy");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name(), "dummy");
    }

    #[test]
    fn registry_get_missing() {
        let registry = ToolRegistry::new();
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn registry_unregister() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));
        registry.unregister("dummy");
        assert!(registry.get("dummy").is_none());
    }

    #[test]
    fn registry_tool_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        let defs = registry.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "dummy");
        assert_eq!(defs[0].description, "A dummy tool for testing");
        assert!(defs[0].input_schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn registry_execute_success() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        let ctx = ToolContext::default();
        let result = registry.execute("dummy", json!({"value": 42}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.content, "got 42");
        assert!(!output.is_error);
    }

    #[tokio::test]
    async fn registry_execute_not_found() {
        let registry = ToolRegistry::new();
        let ctx = ToolContext::default();
        let result = registry.execute("missing", json!({}), &ctx).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::NotFound(name) if name == "missing"));
    }

    #[test]
    fn tool_output_ok() {
        let out = ToolOutput::ok("hello");
        assert_eq!(out.content, "hello");
        assert!(!out.is_error);
    }

    #[test]
    fn tool_output_err() {
        let out = ToolOutput::err("failed");
        assert_eq!(out.content, "failed");
        assert!(out.is_error);
    }

    #[test]
    fn tool_output_with_metadata() {
        let out = ToolOutput::ok("content").with_metadata("exit_code", json!(0));
        assert_eq!(out.metadata.get("exit_code").unwrap(), &json!(0));
    }

    #[test]
    fn tool_output_keeps_short_model_content_inline() {
        let out = ToolOutput::ok("short output").with_limited_model_content();
        assert_eq!(out.content, "short output");
        assert!(out.model_content.is_none());
    }

    #[test]
    fn tool_output_limits_long_model_content() {
        let content = "x".repeat(MODEL_CONTENT_MAX_BYTES * 2);
        let out = ToolOutput::ok(content.clone()).with_limited_model_content();

        assert_eq!(out.content, content);
        let model_content = out.model_content.expect("long output should be limited");
        assert!(model_content.contains("truncated for model context"));
        assert!(model_content.len() < out.content.len());
    }

    #[test]
    fn tool_context_default() {
        let ctx = ToolContext::default();
        assert!(!ctx.working_directory.as_os_str().is_empty());
        assert_eq!(ctx.timeout, Duration::from_secs(30));
        assert!(!ctx.env_vars.is_empty());
    }

    #[test]
    fn tool_error_display() {
        let err = ToolError::InvalidInput("bad arg".into());
        assert_eq!(err.to_string(), "invalid input: bad arg");
    }
}

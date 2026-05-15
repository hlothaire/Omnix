use std::path::PathBuf;

use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;

use crate::memory_store::MemoryStore;

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// Manage persistent memory stores (add/replace/remove entries).
pub struct MemoryTool {
    store_path: PathBuf,
}

impl MemoryTool {
    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        Self {
            store_path: store_path.into(),
        }
    }
}

impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memory entries that survive across sessions. \
         Use this to remember user preferences, environment facts, and lessons learned. \
         Actions: add (append a new entry), replace (find by substring, replace), \
         remove (find by substring, delete)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "replace", "remove"],
                    "description": "The action to perform"
                },
                "content": {
                    "type": "string",
                    "description": "For 'add': the new entry text. For 'replace': the replacement text."
                },
                "old_text": {
                    "type": "string",
                    "description": "For 'replace' and 'remove': a unique substring that identifies exactly one existing entry."
                }
            },
            "required": ["action"],
            "allOf": [
                {
                    "if": { "properties": { "action": { "const": "add" } } },
                    "then": { "required": ["content"] }
                },
                {
                    "if": { "properties": { "action": { "const": "replace" } } },
                    "then": { "required": ["old_text", "content"] }
                },
                {
                    "if": { "properties": { "action": { "const": "remove" } } },
                    "then": { "required": ["old_text"] }
                }
            ]
        })
    }

    fn required_permission(&self) -> PermissionMode {
        PermissionMode::WorkspaceWrite
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> BoxFuture<'_, anyhow::Result<ToolOutput, ToolError>> {
        let action = input["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'action' field".into()))
            .map(|s| s.to_string());
        let content = input["content"].as_str().map(|s| s.to_string());
        let old_text = input["old_text"].as_str().map(|s| s.to_string());
        let path = self.store_path.clone();

        Box::pin(async move {
            let action = action?;
            let mut store = MemoryStore::load(&path)
                .map_err(|e| ToolError::Io(format!("Cannot load memory store: {}", e)))?;

            match action.as_str() {
                "add" => {
                    let content = content.ok_or_else(|| {
                        ToolError::InvalidInput("'add' requires 'content' field".into())
                    })?;
                    store
                        .add(&content)
                        .map_err(|e| ToolError::Other(format!("Cannot add memory: {}", e)))?;
                    let usage = format!("{} entries", store.entries().len());
                    Ok(ToolOutput::ok(format!("Added memory entry: {}", content))
                        .with_metadata("usage", json!(usage)))
                }
                "replace" => {
                    let old = old_text.ok_or_else(|| {
                        ToolError::InvalidInput("'replace' requires 'old_text' field".into())
                    })?;
                    let new = content.ok_or_else(|| {
                        ToolError::InvalidInput("'replace' requires 'content' field".into())
                    })?;
                    store
                        .replace(&old, &new)
                        .map_err(|e| ToolError::Other(format!("Cannot replace memory: {}", e)))?;
                    Ok(
                        ToolOutput::ok(format!("Replaced memory entry matching '{}'", old))
                            .with_metadata("new_content", json!(new)),
                    )
                }
                "remove" => {
                    let old = old_text.ok_or_else(|| {
                        ToolError::InvalidInput("'remove' requires 'old_text' field".into())
                    })?;
                    store
                        .remove(&old)
                        .map_err(|e| ToolError::Other(format!("Cannot remove memory: {}", e)))?;
                    let usage = format!("{} entries", store.entries().len());
                    Ok(
                        ToolOutput::ok(format!("Removed memory entry matching '{}'", old))
                            .with_metadata("usage", json!(usage)),
                    )
                }
                other => Err(ToolError::InvalidInput(format!(
                    "Unknown memory action: '{}'. Use add, replace, or remove.",
                    other
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn memory_tool_add() {
        let dir = tempdir().unwrap();
        let tool = MemoryTool::new(dir.path().join("memory.md"));
        let ctx = ToolContext::default();

        let out = tool
            .execute(json!({"action": "add", "content": "User likes Rust"}), &ctx)
            .await
            .unwrap();

        assert!(out.content.contains("Added"));
        assert!(!out.is_error);
        assert_eq!(out.metadata.get("usage").unwrap(), &json!("1 entries"));
    }

    #[tokio::test]
    async fn memory_tool_replace() {
        let dir = tempdir().unwrap();
        let tool = MemoryTool::new(dir.path().join("memory.md"));
        let ctx = ToolContext::default();

        tool.execute(json!({"action": "add", "content": "User likes Go"}), &ctx)
            .await
            .unwrap();

        let out = tool
            .execute(
                json!({"action": "replace", "old_text": "Go", "content": "User likes Rust"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(out.content.contains("Replaced"));
        assert_eq!(
            out.metadata.get("new_content").unwrap(),
            &json!("User likes Rust")
        );
    }

    #[tokio::test]
    async fn memory_tool_remove() {
        let dir = tempdir().unwrap();
        let tool = MemoryTool::new(dir.path().join("memory.md"));
        let ctx = ToolContext::default();

        tool.execute(json!({"action": "add", "content": "Temp entry"}), &ctx)
            .await
            .unwrap();

        let out = tool
            .execute(json!({"action": "remove", "old_text": "Temp"}), &ctx)
            .await
            .unwrap();

        assert!(out.content.contains("Removed"));
        assert_eq!(out.metadata.get("usage").unwrap(), &json!("0 entries"));
    }

    #[tokio::test]
    async fn memory_tool_invalid_action() {
        let dir = tempdir().unwrap();
        let tool = MemoryTool::new(dir.path().join("memory.md"));
        let ctx = ToolContext::default();

        let result = tool.execute(json!({"action": "delete"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn memory_tool_missing_content_for_add() {
        let dir = tempdir().unwrap();
        let tool = MemoryTool::new(dir.path().join("memory.md"));
        let ctx = ToolContext::default();

        let result = tool.execute(json!({"action": "add"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }
}

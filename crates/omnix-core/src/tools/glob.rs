use std::path::Path;

use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct Glob;

impl Tool for Glob {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. \
         Returns a newline-separated list of matching file paths."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern, e.g. 'src/**/*.rs' or '*.toml'"
                }
            },
            "required": ["pattern"]
        })
    }

    fn required_permission(&self) -> PermissionMode {
        PermissionMode::ReadOnly
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> BoxFuture<'_, anyhow::Result<ToolOutput, ToolError>> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'pattern' field".into()))
            .map(|s| s.to_string());
        let cwd = ctx.working_directory.clone();

        Box::pin(async move {
            let pattern = pattern?;

            let full_pattern = if Path::new(&pattern).is_absolute() {
                pattern
            } else {
                cwd.join(&pattern).to_string_lossy().to_string()
            };

            let entries: Vec<String> = glob::glob(&full_pattern)
                .map_err(|e| ToolError::InvalidInput(format!("Invalid glob pattern: {}", e)))?
                .map(|entry| match entry {
                    Ok(path) => path.to_string_lossy().to_string(),
                    Err(e) => format!("ERR: {}", e),
                })
                .collect();

            if entries.is_empty() {
                Ok(ToolOutput::ok("No files matched the pattern."))
            } else {
                Ok(ToolOutput::ok(entries.join("\n")).with_metadata("count", json!(entries.len())))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn glob_finds_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::write(dir.path().join("b.rs"), "").unwrap();
        fs::write(dir.path().join("c.txt"), "").unwrap();

        let tool = Glob;
        let ctx = ToolContext {
            working_directory: dir.path().to_path_buf(),
            ..Default::default()
        };

        let out = tool
            .execute(json!({"pattern": "*.rs"}), &ctx)
            .await
            .unwrap();

        assert!(out.content.contains("a.rs"));
        assert!(out.content.contains("b.rs"));
        assert!(!out.content.contains("c.txt"));
        assert!(!out.is_error);
        assert_eq!(out.metadata.get("count").unwrap(), &json!(2));
    }

    #[tokio::test]
    async fn glob_no_matches() {
        let dir = tempdir().unwrap();
        let tool = Glob;
        let ctx = ToolContext {
            working_directory: dir.path().to_path_buf(),
            ..Default::default()
        };

        let out = tool
            .execute(json!({"pattern": "*.nonexistent"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("No files matched"));
        assert!(!out.is_error);
    }

    #[tokio::test]
    async fn glob_invalid_pattern() {
        let tool = Glob;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"pattern": "["}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }
}

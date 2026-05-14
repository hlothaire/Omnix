use std::fs;
use std::path::Path;

use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path. \
         Optionally specify an offset (line number) and limit (max lines) to read a subset."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (0-indexed, inclusive)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
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
        let path = resolve_path(&input["path"], &ctx.working_directory);
        let offset = input["offset"].as_u64().map(|n| n as usize);
        let limit = input["limit"].as_u64().map(|n| n as usize);

        Box::pin(async move {
            let path = path?;

            let content = fs::read_to_string(&path)
                .map_err(|e| ToolError::Io(format!("Cannot read {}: {}", path.display(), e)))?;

            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();

            let start = offset.unwrap_or(0).min(total_lines);
            let end = limit
                .map(|l| start + l)
                .unwrap_or(total_lines)
                .min(total_lines);
            let selected: Vec<&str> = lines[start..end].to_vec();

            let mut result = selected.join("\n");
            if end < total_lines {
                result.push_str(&format!(
                    "\n\n... ({} more lines, {} total)",
                    total_lines - end,
                    total_lines
                ));
            }

            Ok(ToolOutput::ok(result).with_metadata("path", json!(path.to_string_lossy())))
        })
    }
}

fn resolve_path(value: &serde_json::Value, cwd: &Path) -> Result<std::path::PathBuf, ToolError> {
    let s = value
        .as_str()
        .ok_or_else(|| ToolError::InvalidInput("Missing 'path' field".into()))?;

    if s.trim().is_empty() {
        return Err(ToolError::InvalidInput("path cannot be empty".into()));
    }

    let path = Path::new(s);

    // Check for protected system paths
    let path_str = path.to_string_lossy();
    let dangerous = ["/etc/passwd", "/etc/shadow", "/etc/hosts"];
    for pattern in &dangerous {
        if path_str.contains(pattern) {
            return Err(ToolError::InvalidInput(format!(
                "cannot access protected system file: {}",
                pattern
            )));
        }
    }

    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(cwd.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn read_file_success() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "line1").unwrap();
        writeln!(tmp, "line2").unwrap();
        writeln!(tmp, "line3").unwrap();

        let tool = ReadFile;
        let ctx = ToolContext {
            working_directory: std::env::temp_dir(),
            ..Default::default()
        };

        let out = tool
            .execute(
                json!({"path": tmp.path().file_name().unwrap().to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(out.content.contains("line1"));
        assert!(out.content.contains("line3"));
        assert!(!out.is_error);
    }

    #[tokio::test]
    async fn read_file_with_offset_limit() {
        let mut tmp = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(tmp, "line{}", i).unwrap();
        }

        let tool = ReadFile;
        let ctx = ToolContext::default();

        let out = tool
            .execute(
                json!({"path": tmp.path().to_str().unwrap(), "offset": 2, "limit": 3}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(out.content.contains("line3"));
        assert!(out.content.contains("line4"));
        assert!(out.content.contains("line5"));
        assert!(!out.content.contains("line6"));
        assert!(out.content.contains("5 more lines"));
    }

    #[tokio::test]
    async fn read_file_not_found() {
        let tool = ReadFile;
        let ctx = ToolContext::default();
        let out = tool
            .execute(json!({"path": "/tmp/nonexistent_file_12345.txt"}), &ctx)
            .await;

        assert!(out.is_err());
    }

    #[tokio::test]
    async fn read_file_empty_path() {
        let tool = ReadFile;
        let ctx = ToolContext::default();
        let out = tool.execute(json!({"path": ""}), &ctx).await;
        assert!(out.is_err());
        assert!(matches!(out.unwrap_err(), ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn read_file_system_path() {
        let tool = ReadFile;
        let ctx = ToolContext::default();
        let out = tool.execute(json!({"path": "/etc/passwd"}), &ctx).await;
        assert!(out.is_err());
        assert!(matches!(out.unwrap_err(), ToolError::InvalidInput(_)));
    }
}

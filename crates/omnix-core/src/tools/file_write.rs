use std::fs;
use std::io::Write;
use std::path::Path;

use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create or overwrite a file with the given content. \
         Parent directories are created automatically."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn required_permission(&self) -> PermissionMode {
        PermissionMode::WorkspaceWrite
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> BoxFuture<'_, anyhow::Result<ToolOutput, ToolError>> {
        let path = resolve_path(&input["path"], &ctx.working_directory);
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'content' field".into()))
            .map(|s| s.to_string());

        Box::pin(async move {
            let path = path?;
            let content = content?;

            // Create parent directories
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Io(format!("Cannot create dirs for {}: {}", path.display(), e))
                })?;
            }

            // Atomic write: temp file then rename
            let tmp_path = path.with_extension("tmp");
            {
                let mut file = fs::File::create(&tmp_path)
                    .map_err(|e| ToolError::Io(format!("Cannot create temp file: {}", e)))?;
                file.write_all(content.as_bytes())
                    .map_err(|e| ToolError::Io(format!("Cannot write temp file: {}", e)))?;
            }

            fs::rename(&tmp_path, &path).map_err(|e| {
                ToolError::Io(format!(
                    "Cannot rename {} to {}: {}",
                    tmp_path.display(),
                    path.display(),
                    e
                ))
            })?;

            Ok(ToolOutput::ok(format!(
                "Wrote {} bytes to {}",
                content.len(),
                path.display()
            ))
            .with_metadata("path", json!(path.to_string_lossy())))
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
    
    // Prevent writing to system files
    let path_str = path.to_string_lossy();
    let dangerous = ["/etc/passwd", "/etc/shadow", "/etc/hosts"];
    for pattern in &dangerous {
        if path_str.contains(pattern) {
            return Err(ToolError::InvalidInput(
                format!("cannot write to protected system file: {}", pattern)
            ));
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
    use tempfile::tempdir;

    #[tokio::test]
    async fn write_file_creates_file() {
        let dir = tempdir().unwrap();
        let tool = WriteFile;
        let mut ctx = ToolContext::default();
        ctx.working_directory = dir.path().to_path_buf();

        let out = tool
            .execute(json!({"path": "test.txt", "content": "hello world"}), &ctx)
            .await
            .unwrap();

        assert!(out.content.contains("Wrote 11 bytes"));
        assert!(!out.is_error);

        let read = fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(read, "hello world");
    }

    #[tokio::test]
    async fn write_file_creates_parents() {
        let dir = tempdir().unwrap();
        let tool = WriteFile;
        let mut ctx = ToolContext::default();
        ctx.working_directory = dir.path().to_path_buf();

        let out = tool
            .execute(json!({"path": "a/b/c/deep.txt", "content": "deep"}), &ctx)
            .await
            .unwrap();

        assert!(!out.is_error);
        assert!(dir.path().join("a/b/c/deep.txt").exists());
    }

    #[tokio::test]
    async fn write_file_overwrites() {
        let dir = tempdir().unwrap();
        let tool = WriteFile;
        let mut ctx = ToolContext::default();
        ctx.working_directory = dir.path().to_path_buf();

        fs::write(dir.path().join("existing.txt"), "old").unwrap();

        let out = tool
            .execute(json!({"path": "existing.txt", "content": "new"}), &ctx)
            .await
            .unwrap();

        assert!(!out.is_error);
        let read = fs::read_to_string(dir.path().join("existing.txt")).unwrap();
        assert_eq!(read, "new");
    }

    #[tokio::test]
    async fn write_file_invalid_input() {
        let tool = WriteFile;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"path": "foo.txt"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn write_file_empty_path() {
        let tool = WriteFile;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"path": "", "content": "test"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn write_file_system_path() {
        let tool = WriteFile;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"path": "/etc/passwd", "content": "x"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }
}

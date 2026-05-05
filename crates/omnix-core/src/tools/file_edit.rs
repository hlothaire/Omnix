use std::fs;
use std::path::Path;

use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Apply a search/replace edit to an existing file. \
         The old_string must match exactly one location in the file. \
         Uses atomic write (temp file + rename)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to search for. Must match exactly one location."
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text"
                }
            },
            "required": ["path", "old_string", "new_string"]
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
        let old_str = input["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'old_string' field".into()))
            .map(|s| s.to_string());
        let new_str = input["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'new_string' field".into()))
            .map(|s| s.to_string());

        Box::pin(async move {
            let path = path?;
            let old_str = old_str?;
            let new_str = new_str?;

            let content = fs::read_to_string(&path)
                .map_err(|e| ToolError::Io(format!("Cannot read {}: {}", path.display(), e)))?;

            let match_count = content.matches(&old_str).count();

            if match_count == 0 {
                return Err(ToolError::InvalidInput(format!(
                    "old_string not found in {}",
                    path.display()
                )));
            }

            if match_count > 1 {
                return Err(ToolError::InvalidInput(format!(
                    "old_string matches {} locations in {} — must be unique",
                    match_count,
                    path.display()
                )));
            }

            let new_content = content.replacen(&old_str, &new_str, 1);

            // Atomic write
            let tmp_path = path.with_extension("tmp");
            fs::write(&tmp_path, new_content.as_bytes())
                .map_err(|e| ToolError::Io(format!("Cannot write temp file: {}", e)))?;
            fs::rename(&tmp_path, &path)
                .map_err(|e| ToolError::Io(format!("Cannot rename temp file: {}", e)))?;

            Ok(ToolOutput::ok(format!(
                "Edited {}: replaced {} chars with {} chars",
                path.display(),
                old_str.len(),
                new_str.len()
            ))
            .with_metadata("path", json!(path.to_string_lossy())))
        })
    }
}

fn resolve_path(value: &serde_json::Value, cwd: &Path) -> Result<std::path::PathBuf, ToolError> {
    let s = value
        .as_str()
        .ok_or_else(|| ToolError::InvalidInput("Missing 'path' field".into()))?;
    let path = Path::new(s);
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
    async fn edit_file_success() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "foo bar baz").unwrap();

        let tool = EditFile;
        let ctx = ToolContext::default();

        let out = tool
            .execute(
                json!({
                    "path": tmp.path().to_str().unwrap(),
                    "old_string": "bar",
                    "new_string": "qux"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!out.is_error);
        let content = fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(content, "foo qux baz");
    }

    #[tokio::test]
    async fn edit_file_not_found() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "foo bar baz").unwrap();

        let tool = EditFile;
        let ctx = ToolContext::default();

        let result = tool
            .execute(
                json!({
                    "path": tmp.path().to_str().unwrap(),
                    "old_string": "missing",
                    "new_string": "x"
                }),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn edit_file_multiple_matches() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "foo foo foo").unwrap();

        let tool = EditFile;
        let ctx = ToolContext::default();

        let result = tool
            .execute(
                json!({
                    "path": tmp.path().to_str().unwrap(),
                    "old_string": "foo",
                    "new_string": "bar"
                }),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }
}

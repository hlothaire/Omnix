use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List directory contents with metadata (size, modification time, type). Supports recursive tree view."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (default: current directory)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Include subdirectories in tree view (default: false)"
                }
            }
        })
    }

    fn required_permission(&self) -> PermissionMode {
        PermissionMode::ReadOnly
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
        let path_str = input["path"].as_str().unwrap_or(".").to_string();
        let recursive = input["recursive"].as_bool().unwrap_or(false);
        let base_dir = ctx.working_directory.clone();

        Box::pin(async move {
            let target_path = if Path::new(&path_str).is_absolute() {
                PathBuf::from(&path_str)
            } else {
                base_dir.join(&path_str)
            };

            if !target_path.exists() {
                return Err(ToolError::Io(format!(
                    "path does not exist: {}",
                    target_path.display()
                )));
            }

            if !target_path.is_dir() {
                return Err(ToolError::Io(format!(
                    "path is not a directory: {}",
                    target_path.display()
                )));
            }

            let mut lines = Vec::new();

            if recursive {
                lines.push(target_path.display().to_string());
                list_recursive(&target_path, "", &mut lines)?;
            } else {
                list_flat(&target_path, &mut lines)?;
            }

            Ok(ToolOutput::ok(lines.join("\n")))
        })
    }
}

fn list_flat(dir: &Path, lines: &mut Vec<String>) -> Result<(), ToolError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| ToolError::Io(e.to_string()))?
        .filter_map(|e| e.ok())
        .collect();

    // Sort: directories first, then files, alphabetically
    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for entry in entries {
        let metadata = entry.metadata().map_err(|e| ToolError::Io(e.to_string()))?;
        let file_type = if metadata.is_dir() {
            "d"
        } else if metadata.is_symlink() {
            "l"
        } else {
            "-"
        };

        let size = format_size(metadata.len());
        let modified = format_time(metadata.modified().ok());
        let name = entry.file_name().to_string_lossy().to_string();

        lines.push(format!(
            "{}{:>10}  {:>19}  {}",
            file_type, size, modified, name
        ));
    }

    Ok(())
}

fn list_recursive(dir: &Path, prefix: &str, lines: &mut Vec<String>) -> Result<(), ToolError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| ToolError::Io(e.to_string()))?
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == count - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata().map_err(|e| ToolError::Io(e.to_string()))?;
        let file_type = if metadata.is_dir() {
            "d"
        } else if metadata.is_symlink() {
            "l"
        } else {
            "-"
        };

        lines.push(format!(
            "{}{}{}{}  {}",
            prefix,
            connector,
            file_type,
            if metadata.is_dir() { "/" } else { " " },
            name
        ));

        if metadata.is_dir() {
            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };
            list_recursive(&entry.path(), &child_prefix, lines)?;
        }
    }

    Ok(())
}

fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = size as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

fn format_time(time: Option<std::time::SystemTime>) -> String {
    use chrono::{DateTime, Local};

    match time {
        Some(t) => {
            let datetime: DateTime<Local> = t.into();
            datetime.format("%Y-%m-%d %H:%M").to_string()
        }
        None => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_dir_current() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();

        let list_dir = ListDir;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = list_dir.execute(json!({}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("subdir"));
        assert!(output.content.contains("file.txt"));
        assert!(output.content.contains("d")); // directory marker
        assert!(output.content.contains("-")); // file marker
    }

    #[tokio::test]
    async fn test_list_dir_recursive() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("a")).unwrap();
        fs::write(tmp.path().join("a").join("b.txt"), "hello").unwrap();

        let list_dir = ListDir;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = list_dir.execute(json!({"recursive": true}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("a"));
        assert!(output.content.contains("b.txt"));
        assert!(output.content.contains("├──") || output.content.contains("└──"));
    }

    #[tokio::test]
    async fn test_list_dir_not_found() {
        let list_dir = ListDir;
        let ctx = ToolContext::default();

        let result = list_dir
            .execute(json!({"path": "nonexistent"}), &ctx)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::Io(_)));
    }

    #[tokio::test]
    async fn test_list_dir_file_not_dir() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello").unwrap();

        let list_dir = ListDir;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = list_dir
            .execute(json!({"path": "file.txt"}), &ctx)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::Io(_)));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }
}

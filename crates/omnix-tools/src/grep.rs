use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use regex::Regex;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct Grep;

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents by regex pattern. Returns matching lines with file path and line number."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in (default: current directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files, e.g. '*.rs' (default: all files)"
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
    ) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
        let pattern = input["pattern"].as_str().unwrap_or("").to_string();
        let path_str = input["path"].as_str().unwrap_or(".").to_string();
        let glob_filter = input["glob"].as_str().map(|s| s.to_string());

        let base_dir = ctx.working_directory.clone();

        Box::pin(async move {
            if pattern.is_empty() {
                return Err(ToolError::InvalidInput("pattern is required".to_string()));
            }

            let regex = match Regex::new(&pattern) {
                Ok(re) => re,
                Err(e) => {
                    return Err(ToolError::InvalidInput(format!(
                        "invalid regex pattern '{}': {}",
                        pattern, e
                    )));
                }
            };

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

            let mut results = Vec::new();

            if target_path.is_file() {
                search_file(&target_path, &regex, &mut results)?;
            } else {
                search_directory(&target_path, &regex, glob_filter.as_deref(), &mut results)?;
            }

            if results.is_empty() {
                return Ok(ToolOutput::ok("No matches found."));
            }

            Ok(ToolOutput::ok(results.join("\n")))
        })
    }
}

fn search_file(path: &Path, regex: &Regex, results: &mut Vec<String>) -> Result<(), ToolError> {
    let metadata = fs::metadata(path).map_err(|e| ToolError::Io(e.to_string()))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Ok(()); // Skip large files silently
    }

    let contents = fs::read(path).map_err(|e| ToolError::Io(e.to_string()))?;

    // Skip binary files (check for null bytes in first 8KB)
    let check_len = contents.len().min(8192);
    if contents[..check_len].contains(&0) {
        return Ok(());
    }

    let text = String::from_utf8_lossy(&contents);

    for (line_num, line) in text.lines().enumerate() {
        if regex.is_match(line) {
            results.push(format!("{}:{}:{}", path.display(), line_num + 1, line));
        }
    }

    Ok(())
}

fn search_directory(
    dir: &Path,
    regex: &Regex,
    glob_filter: Option<&str>,
    results: &mut Vec<String>,
) -> Result<(), ToolError> {
    let entries = fs::read_dir(dir).map_err(|e| ToolError::Io(e.to_string()))?;

    for entry in entries {
        let entry = entry.map_err(|e| ToolError::Io(e.to_string()))?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            search_directory(&path, regex, glob_filter, results)?;
        } else if path.is_file() {
            // Check glob filter
            if let Some(glob) = glob_filter {
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !glob_match(glob, file_name) {
                    continue;
                }
            }

            search_file(&path, regex, results)?;
        }
    }

    Ok(())
}

fn glob_match(pattern: &str, name: &str) -> bool {
    // Simple glob matching: * matches anything, ? matches single char
    let mut chars = pattern.chars().peekable();
    let mut name_chars = name.chars().peekable();

    while let Some(p) = chars.next() {
        match p {
            '*' => {
                // Match zero or more characters
                if chars.peek().is_none() {
                    return true; // * at end matches everything
                }
                let next = *chars.peek().unwrap();
                while let Some(&nc) = name_chars.peek() {
                    if nc == next {
                        break;
                    }
                    name_chars.next();
                }
            }
            '?' => {
                name_chars.next();
            }
            c => match name_chars.next() {
                Some(nc) if nc == c => {}
                _ => return false,
            },
        }
    }

    name_chars.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_grep_single_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.rs");
        fs::write(&file, "fn main() {}\nfn helper() {}\n").unwrap();

        let grep = Grep;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = grep
            .execute(json!({"pattern": "fn main", "path": "test.rs"}), &ctx)
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("fn main"));
        assert!(output.content.contains("test.rs:1:"));
    }

    #[tokio::test]
    async fn test_grep_directory() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        fs::write(tmp.path().join("b.rs"), "fn helper() {}\n").unwrap();

        let grep = Grep;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = grep.execute(json!({"pattern": "fn main"}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("a.rs:"));
        assert!(!output.content.contains("b.rs:"));
    }

    #[tokio::test]
    async fn test_grep_with_glob() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.rs"), "fn main() {}\n").unwrap();
        fs::write(tmp.path().join("test.toml"), "fn main() {}\n").unwrap();

        let grep = Grep;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = grep
            .execute(json!({"pattern": "fn main", "glob": "*.rs"}), &ctx)
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("test.rs:"));
        assert!(!output.content.contains("test.toml:"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.rs"), "fn helper() {}\n").unwrap();

        let grep = Grep;
        let ctx = ToolContext {
            working_directory: tmp.path().to_path_buf(),
            ..ToolContext::default()
        };

        let result = grep
            .execute(json!({"pattern": "fn main", "path": "test.rs"}), &ctx)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().content, "No matches found.");
    }

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let grep = Grep;
        let ctx = ToolContext::default();

        let result = grep.execute(json!({"pattern": "[invalid"}), &ctx).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_grep_file_not_found() {
        let grep = Grep;
        let ctx = ToolContext::default();

        let result = grep
            .execute(json!({"pattern": "test", "path": "nonexistent"}), &ctx)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::Io(_)));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "Cargo.toml"));
        assert!(glob_match("test_*.rs", "test_main.rs"));
        assert!(!glob_match("test_*.rs", "main.rs"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("?est.rs", "test.rs"));
    }
}

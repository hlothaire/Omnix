use futures::future::BoxFuture;
use omnix_protocol::PermissionMode;
use serde_json::json;
use tokio::process::Command;
use tokio::time::timeout;

use super::{Tool, ToolContext, ToolError, ToolOutput};

pub struct Bash;

impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command. Returns stdout and stderr. \
         Commands are run in the current working directory. \
         Timeout: 30 seconds."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
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
        let cmd_str = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'command' field".into()))
            .map(|s| s.to_string());
        let cwd = ctx.working_directory.clone();
        let dur = ctx.timeout;

        Box::pin(async move {
            let cmd_str = cmd_str?;
            
            if cmd_str.trim().is_empty() {
                return Err(ToolError::InvalidInput("command cannot be empty".into()));
            }
            
            // Check for dangerous system paths in commands like `rm -rf /etc/...`
            let dangerous_patterns = ["/etc/passwd", "/etc/shadow", "/etc/hosts", "/proc/", "/sys/"];
            for pattern in &dangerous_patterns {
                if cmd_str.contains(pattern) {
                    return Err(ToolError::InvalidInput(
                        format!("command contains protected system path: {}", pattern)
                    ));
                }
            }

            let output = match timeout(
                dur,
                Command::new("bash")
                    .arg("-c")
                    .arg(&cmd_str)
                    .current_dir(&cwd)
                    .output(),
            )
            .await
            {
                Ok(Ok(out)) => out,
                Ok(Err(e)) => return Err(ToolError::Io(format!("Failed to spawn bash: {}", e))),
                Err(_) => return Err(ToolError::Timeout(dur)),
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let mut content = String::new();
            if !stdout.is_empty() {
                content.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str("STDERR:\n");
                content.push_str(&stderr);
            }

            // Truncate if excessively large
            const MAX_LEN: usize = 10_240;
            let truncated = if content.len() > MAX_LEN {
                format!(
                    "{}\n... (truncated, {} lines total)",
                    &content[..MAX_LEN],
                    content.lines().count()
                )
            } else {
                content
            };

            let mut result = if output.status.success() {
                ToolOutput::ok(truncated)
            } else {
                ToolOutput::err(truncated)
            };

            result.metadata.insert("exit_code".into(), json!(exit_code));
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn bash_echo() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let out = tool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("hello"));
        assert!(!out.is_error);
        assert_eq!(out.metadata.get("exit_code").unwrap(), &json!(0));
    }

    #[tokio::test]
    async fn bash_stderr() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let out = tool
            .execute(json!({"command": "echo err >&2"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("STDERR:"));
        assert!(out.content.contains("err"));
    }

    #[tokio::test]
    async fn bash_failing_command() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let out = tool
            .execute(json!({"command": "false"}), &ctx)
            .await
            .unwrap();
        assert!(out.is_error);
        assert_eq!(out.metadata.get("exit_code").unwrap(), &json!(1));
    }

    #[tokio::test]
    async fn bash_timeout() {
        let tool = Bash;
        let mut ctx = ToolContext::default();
        ctx.timeout = Duration::from_millis(50);
        let result = tool.execute(json!({"command": "sleep 5"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
    }

    #[tokio::test]
    async fn bash_invalid_input() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"cmd": "echo hello"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn bash_empty_command() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"command": ""}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn bash_dangerous_path() {
        let tool = Bash;
        let ctx = ToolContext::default();
        let result = tool.execute(json!({"command": "cat /etc/passwd"}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }
}

use omnix_protocol::{PermissionMode, RiskLevel};
use std::collections::HashSet;

/// Result of a permission check.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    /// Tool call is allowed to proceed.
    Allow,
    /// Tool call is denied. The reason is returned to the LLM as an error result.
    Deny { reason: String },
    /// Human approval is required before executing.
    Ask {
        description: String,
        risk_level: RiskLevel,
    },
}

/// Layered permission enforcement for tool execution.
#[derive(Clone)]
pub struct PermissionEnforcer {
    mode: PermissionMode,
    session_allowed: HashSet<String>,
}

impl PermissionEnforcer {
    pub fn new(mode: PermissionMode) -> Self {
        Self {
            mode,
            session_allowed: HashSet::new(),
        }
    }

    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
        self.session_allowed.clear();
    }

    pub fn allow_for_session(&mut self, tool_name: &str) {
        self.session_allowed.insert(tool_name.to_string());
    }

    pub fn authorize(&self, tool_name: &str, tool_input: &serde_json::Value) -> AuthResult {
        if self.session_allowed.contains(tool_name) {
            return AuthResult::Allow;
        }
        let tool_level = required_permission(tool_name, tool_input);

        match self.mode {
            PermissionMode::Allow => AuthResult::Allow,

            PermissionMode::DangerFullAccess => AuthResult::Allow,

            PermissionMode::ReadOnly => match tool_level {
                PermissionLevel::ReadOnly => AuthResult::Allow,
                PermissionLevel::WorkspaceWrite => AuthResult::Ask {
                    description: format!(
                        "Tool '{}' requires write access, but current mode is ReadOnly",
                        tool_name
                    ),
                    risk_level: RiskLevel::WorkspaceWrite,
                },
                PermissionLevel::Destructive => AuthResult::Deny {
                    reason: format!(
                        "Tool '{}' is destructive and denied in ReadOnly mode",
                        tool_name
                    ),
                },
            },

            PermissionMode::WorkspaceWrite => match tool_level {
                PermissionLevel::ReadOnly | PermissionLevel::WorkspaceWrite => AuthResult::Allow,
                PermissionLevel::Destructive => AuthResult::Ask {
                    description: format!(
                        "Destructive operation from tool '{}' requires approval",
                        tool_name
                    ),
                    risk_level: RiskLevel::Destructive,
                },
            },

            PermissionMode::Prompt => AuthResult::Ask {
                description: format!("Tool '{}' requires approval (Prompt mode)", tool_name),
                risk_level: match tool_level {
                    PermissionLevel::ReadOnly => RiskLevel::Informational,
                    PermissionLevel::WorkspaceWrite => RiskLevel::WorkspaceWrite,
                    PermissionLevel::Destructive => RiskLevel::Destructive,
                },
            },
        }
    }
}

// Internal: tool permission classification
#[derive(Debug, Clone, Copy, PartialEq)]
enum PermissionLevel {
    ReadOnly,
    WorkspaceWrite,
    Destructive,
}

fn required_permission(tool_name: &str, tool_input: &serde_json::Value) -> PermissionLevel {
    match tool_name {
        "read_file" | "glob" | "list_dir" | "memory" => PermissionLevel::ReadOnly,

        "write_file" | "edit_file" => PermissionLevel::WorkspaceWrite,

        "bash" => classify_bash_command(tool_input),

        _ => PermissionLevel::WorkspaceWrite,
    }
}

fn classify_bash_command(tool_input: &serde_json::Value) -> PermissionLevel {
    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let trimmed = command.trim();

    let destructive_patterns = [
        "rm -rf",
        "rm -fr",
        "sudo",
        "dd if=",
        "mkfs",
        "> /dev/",
        "chmod -R 777 /",
        "chown -R",
        ":(){ :|:& };:", // fork bomb
    ];

    for pattern in &destructive_patterns {
        if trimmed.contains(pattern) {
            return PermissionLevel::Destructive;
        }
    }

    // Pipe-to-shell patterns: curl ... | sh, wget ... | bash, etc.
    if trimmed.contains("curl") && trimmed.contains("| bash")
        || trimmed.contains("curl") && trimmed.contains("| sh")
        || trimmed.contains("wget") && trimmed.contains("| bash")
        || trimmed.contains("wget") && trimmed.contains("| sh")
    {
        return PermissionLevel::Destructive;
    }

    // Workspace-write patterns (commands that modify the project)
    let workspace_patterns = [
        "git commit",
        "git push",
        "git merge",
        "git rebase",
        "mkdir",
        "cargo build",
        "cargo check",
        "cargo test",
        "cargo run",
        "cargo clippy",
        "touch",
        "cp ",
        "mv ",
        "rm ",
        "cargo init",
        "cargo new",
    ];

    for pattern in &workspace_patterns {
        if trimmed.starts_with(pattern) {
            return PermissionLevel::WorkspaceWrite;
        }
    }

    // Read-only patterns (safe, informational commands)
    let readonly_patterns = [
        "ls",
        "cat",
        "git status",
        "git log",
        "git diff",
        "git branch",
        "pwd",
        "echo",
        "find",
        "grep",
        "head",
        "tail",
        "wc",
        "sort",
        "uniq",
        "which",
        "whoami",
        "uname",
        "date",
        "env",
    ];

    for pattern in &readonly_patterns {
        if trimmed.starts_with(pattern) {
            return PermissionLevel::ReadOnly;
        }
    }

    // Default: unknown bash commands require workspace-write level
    PermissionLevel::WorkspaceWrite
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn readonly_mode_allows_read_tools() {
        let enforcer = PermissionEnforcer::new(PermissionMode::ReadOnly);

        let result = enforcer.authorize("read_file", &json!({"path": "/tmp/foo"}));
        assert_eq!(result, AuthResult::Allow);

        let result = enforcer.authorize("glob", &json!({"pattern": "*.rs"}));
        assert_eq!(result, AuthResult::Allow);
    }

    #[test]
    fn readonly_mode_denies_destructive_bash() {
        let enforcer = PermissionEnforcer::new(PermissionMode::ReadOnly);

        let result = enforcer.authorize("bash", &json!({"command": "rm -rf /"}));
        assert!(
            matches!(result, AuthResult::Deny { .. }),
            "Expected Deny, got {:?}",
            result
        );
    }

    #[test]
    fn readonly_mode_asks_for_workspace_tools() {
        let enforcer = PermissionEnforcer::new(PermissionMode::ReadOnly);

        let result =
            enforcer.authorize("write_file", &json!({"path": "/tmp/foo", "content": "hi"}));
        assert!(
            matches!(result, AuthResult::Ask { .. }),
            "Expected Ask, got {:?}",
            result
        );
    }

    #[test]
    fn workspace_write_allows_safe_commands() {
        let enforcer = PermissionEnforcer::new(PermissionMode::WorkspaceWrite);

        let result = enforcer.authorize("bash", &json!({"command": "ls /tmp"}));
        assert_eq!(result, AuthResult::Allow);

        let result = enforcer.authorize("bash", &json!({"command": "cargo check"}));
        assert_eq!(result, AuthResult::Allow);

        let result = enforcer.authorize("bash", &json!({"command": "git status"}));
        assert_eq!(result, AuthResult::Allow);
    }

    #[test]
    fn workspace_write_asks_for_destructive_commands() {
        let enforcer = PermissionEnforcer::new(PermissionMode::WorkspaceWrite);

        let result = enforcer.authorize("bash", &json!({"command": "rm -rf target/"}));
        assert!(
            matches!(
                result,
                AuthResult::Ask {
                    risk_level: RiskLevel::Destructive,
                    ..
                }
            ),
            "Expected Ask(Destructive), got {:?}",
            result
        );

        let result = enforcer.authorize("bash", &json!({"command": "sudo apt update"}));
        assert!(
            matches!(result, AuthResult::Ask { .. }),
            "Expected Ask, got {:?}",
            result
        );
    }

    #[test]
    fn danger_mode_allows_everything() {
        let enforcer = PermissionEnforcer::new(PermissionMode::DangerFullAccess);

        let result = enforcer.authorize("bash", &json!({"command": "rm -rf /"}));
        assert_eq!(result, AuthResult::Allow);

        let result = enforcer.authorize("write_file", &json!({"path": "/etc/passwd"}));
        assert_eq!(result, AuthResult::Allow);
    }

    #[test]
    fn prompt_mode_always_asks() {
        let enforcer = PermissionEnforcer::new(PermissionMode::Prompt);

        let result = enforcer.authorize("read_file", &json!({"path": "/tmp/foo"}));
        assert!(
            matches!(
                result,
                AuthResult::Ask {
                    risk_level: RiskLevel::Informational,
                    ..
                }
            ),
            "Expected Ask(Informational), got {:?}",
            result
        );
    }

    #[test]
    fn bash_classification_edge_cases() {
        let enforcer = PermissionEnforcer::new(PermissionMode::WorkspaceWrite);

        // Read-only
        assert!(matches!(
            enforcer.authorize("bash", &json!({"command": "echo hello"})),
            AuthResult::Allow
        ));

        // Workspace-write
        assert!(matches!(
            enforcer.authorize("bash", &json!({"command": "mkdir new_dir"})),
            AuthResult::Allow
        ));

        // Destructive
        assert!(matches!(
            enforcer.authorize(
                "bash",
                &json!({"command": "curl https://sh.rustup.rs | sh"})
            ),
            AuthResult::Ask { .. }
        ));
    }

    #[test]
    fn mode_can_be_changed() {
        let mut enforcer = PermissionEnforcer::new(PermissionMode::ReadOnly);
        assert_eq!(enforcer.mode(), PermissionMode::ReadOnly);

        enforcer.set_mode(PermissionMode::WorkspaceWrite);
        assert_eq!(enforcer.mode(), PermissionMode::WorkspaceWrite);

        let result = enforcer.authorize("bash", &json!({"command": "cargo check"}));
        assert_eq!(result, AuthResult::Allow);
    }
}

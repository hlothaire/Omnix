use omnix_protocol::{PermissionMode, ToolDefinition};

pub struct SystemPromptBuilder {
    base_identity: String,
    permission_mode: PermissionMode,
    tool_descriptions: Vec<ToolDefinition>,
    memory_snapshot: Option<String>,
}

impl SystemPromptBuilder {
    pub fn new(permission_mode: PermissionMode) -> Self {
        Self {
            base_identity: default_identity(),
            permission_mode,
            tool_descriptions: Vec::new(),
            memory_snapshot: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tool_descriptions = tools;
        self
    }

    pub fn with_memory_snapshot(mut self, snapshot: Option<String>) -> Self {
        self.memory_snapshot = snapshot;
        self
    }

    /// Build the complete system prompt string.
    pub fn build(&self) -> String {
        let mut parts = Vec::new();

        // 1. Base identity
        parts.push(self.base_identity.clone());

        // 2. Permission / mode notice
        parts.push(format_permission_notice(self.permission_mode));

        // 3. Memory snapshot (if any)
        if let Some(snapshot) = &self.memory_snapshot
            && !snapshot.is_empty()
        {
            parts.push(format_memory_section(snapshot));
        }

        // 4. Tool usage instructions
        parts.push(tool_usage_instructions().to_string());

        // 5. Auto-generated tool descriptions
        parts.push(self.format_tool_descriptions());

        parts.join("\n\n")
    }

    fn format_tool_descriptions(&self) -> String {
        let mut lines = vec!["## Available Tools".to_string()];

        for tool in &self.tool_descriptions {
            lines.push(format!("### {}", tool.name));
            lines.push(tool.description.clone());

            // Inject the JSON schema as the "Parameters" block
            let schema_str = serde_json::to_string_pretty(&tool.input_schema)
                .unwrap_or_else(|_| "{}".to_string());
            lines.push(format!("Parameters:\n```json\n{}\n```", schema_str));
        }

        lines.join("\n\n")
    }
}

fn default_identity() -> String {
    r#"# Identity

You are Omnix, an interactive agentic assistant running in a terminal.
You help the user read, write, and edit files, run shell commands, and manage projects.

You operate in a local-first environment using a local LLM via llama.cpp.
"#
    .to_string()
}

fn format_permission_notice(mode: PermissionMode) -> String {
    match mode {
        PermissionMode::ReadOnly => r#"# Current Permission Mode: ReadOnly

You are restricted to read-only operations.
You may use: read_file, glob, list_dir, grep.
Any request to write or modify files must be refused.
"#
        .to_string(),
        PermissionMode::WorkspaceWrite => r#"# Current Permission Mode: WorkspaceWrite

You may read files and write/modify files within the current workspace.
You may run safe shell commands (e.g. ls, cargo check, git status).
Destructive operations (rm -rf, sudo, etc.) require explicit user approval.
"#
        .to_string(),
        PermissionMode::DangerFullAccess => r#"# Current Permission Mode: DangerFullAccess

WARNING: You have full system access.
All tool calls are auto-approved. Be extremely careful.
"#
        .to_string(),
        PermissionMode::Prompt => r#"# Current Permission Mode: Prompt

Every tool call requires explicit user approval before execution.
Plan your actions and wait for confirmation.
"#
        .to_string(),
        PermissionMode::Allow => r#"# Current Permission Mode: Allow

All tool calls are auto-approved. Proceed with confidence.
"#
        .to_string(),
    }
}

fn format_memory_section(snapshot: &str) -> String {
    format!(
        r#"# Persistent Memory

The following facts have been remembered from previous sessions. Use them to
provide better, more personalized assistance.

{}
"#,
        snapshot
    )
}

fn tool_usage_instructions() -> &'static str {
    r#"# Tool Usage Instructions

When you need to take an action, use a tool call. Tool calls are formatted as
JSON function invocations.

Rules:
- Use tools proactively. Do not ask the user for permission unless the tool
  requires approval (you will be told if it does).
- Combine multiple tool calls in a single turn when they are independent.
- Wait for tool results before making the next set of calls.
- If a tool returns an error, analyze it and try a different approach.
- Keep file reads focused: use offset/limit for large files.
- For edits, prefer `edit_file` (search/replace) over `write_file` when
  modifying existing files, to avoid overwriting unrelated content.

Response format:
You may emit text reasoning followed by one or more tool calls. When you call a
tool, output it as a structured function call (the exact format depends on the
LLM; the harness will parse it).
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnix_protocol::{PermissionMode, ToolDefinition};
    use serde_json::json;

    #[test]
    fn prompt_contains_identity() {
        let builder = SystemPromptBuilder::new(PermissionMode::WorkspaceWrite);
        let prompt = builder.build();
        assert!(prompt.contains("Omnix"));
        assert!(prompt.contains("interactive agentic assistant"));
    }

    #[test]
    fn prompt_contains_permission_notice() {
        let builder = SystemPromptBuilder::new(PermissionMode::ReadOnly);
        let prompt = builder.build();
        assert!(prompt.contains("ReadOnly"));
        assert!(prompt.contains("restricted to read-only"));
    }

    #[test]
    fn prompt_contains_danger_warning() {
        let builder = SystemPromptBuilder::new(PermissionMode::DangerFullAccess);
        let prompt = builder.build();
        assert!(prompt.contains("WARNING"));
        assert!(prompt.contains("full system access"));
    }

    #[test]
    fn prompt_includes_tool_descriptions() {
        let tools = vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read the contents of a file".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }),
                required_permission: PermissionMode::ReadOnly,
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Run a shell command".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"]
                }),
                required_permission: PermissionMode::WorkspaceWrite,
            },
        ];

        let builder = SystemPromptBuilder::new(PermissionMode::WorkspaceWrite).with_tools(tools);
        let prompt = builder.build();

        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("### read_file"));
        assert!(prompt.contains("### bash"));
        assert!(prompt.contains("Read the contents of a file"));
        assert!(prompt.contains("Run a shell command"));
        // JSON schema should be embedded
        assert!(prompt.contains("\"type\": \"object\""));
        assert!(prompt.contains("\"path\""));
    }

    #[test]
    fn prompt_includes_memory_snapshot() {
        let snapshot = "- User prefers concise responses\n- Project uses Rust + Iced".to_string();
        let builder = SystemPromptBuilder::new(PermissionMode::WorkspaceWrite)
            .with_memory_snapshot(Some(snapshot));
        let prompt = builder.build();

        assert!(prompt.contains("Persistent Memory"));
        assert!(prompt.contains("User prefers concise responses"));
        assert!(prompt.contains("Project uses Rust + Iced"));
    }

    #[test]
    fn prompt_omits_empty_memory() {
        let builder = SystemPromptBuilder::new(PermissionMode::WorkspaceWrite)
            .with_memory_snapshot(Some(String::new()));
        let prompt = builder.build();
        assert!(!prompt.contains("Persistent Memory"));
    }

    #[test]
    fn prompt_omits_none_memory() {
        let builder =
            SystemPromptBuilder::new(PermissionMode::WorkspaceWrite).with_memory_snapshot(None);
        let prompt = builder.build();
        assert!(!prompt.contains("Persistent Memory"));
    }

    #[test]
    fn prompt_contains_tool_usage_instructions() {
        let builder = SystemPromptBuilder::new(PermissionMode::WorkspaceWrite);
        let prompt = builder.build();
        assert!(prompt.contains("Tool Usage Instructions"));
        assert!(prompt.contains("Use tools proactively"));
    }
}

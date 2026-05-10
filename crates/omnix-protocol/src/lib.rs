pub mod command;
pub mod enums;
pub mod event;
pub mod memory;
pub mod message;
pub mod tool;
pub mod types;

pub use command::CoreCommand;
pub use enums::{
    ApprovalResponse, MemoryAction, MemoryTarget, PermissionMode, ProviderKind, RiskLevel,
    StopReason,
};
pub use event::CoreEvent;
pub use memory::{MemoryConfig, MemoryEntry, MemoryResult};
pub use message::{ChatMessage, ContentBlock, Role};
pub use tool::{ToolChoice, ToolDefinition};
pub use types::{TokenUsage, ToolResultContent};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrip() {
        let event = CoreEvent::TokenDelta { text: "hello".into() };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: CoreEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn command_roundtrip() {
        let cmd = CoreCommand::SendPrompt { text: "list files".into() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"send_prompt\""));
        let decoded: CoreCommand = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn approval_requested_json() {
        let event = CoreEvent::ApprovalRequested {
            call_id: "call_1".into(),
            tool_name: "bash".into(),
            tool_input: serde_json::json!({"command": "rm -rf /tmp"}),
            risk_level: RiskLevel::Destructive,
            description: "Destructive command".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"approval_requested\""));
        assert!(json.contains("\"risk_level\":\"destructive\""));
    }

    #[test]
    fn content_block_tagged_format() {
        let block = ContentBlock::Text { text: "hi".into() };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
    }

    #[test]
    fn chat_message_user() {
        let msg = ChatMessage::user("hello");
        assert_eq!(msg.role, Role::User);
        assert!(matches!(&msg.content[0], ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn tool_result_default_is_error() {
        let json = r#"{"type":"tool_result","tool_use_id":"id1","content":"ok"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::ToolResult { is_error, .. } => assert!(!is_error),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn token_usage_merge() {
        let mut a = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 2,
            cache_read_tokens: 0,
        };
        let b = TokenUsage {
            input_tokens: 20,
            output_tokens: 15,
            cache_creation_tokens: 3,
            cache_read_tokens: 1,
        };
        a.merge(&b);
        assert_eq!(a.input_tokens, 30);
        assert_eq!(a.output_tokens, 20);
        assert_eq!(a.cache_creation_tokens, 5);
        assert_eq!(a.cache_read_tokens, 1);
    }

    #[test]
    fn memory_action_add_roundtrip() {
        let action = MemoryAction::Add {
            content: "User prefers dark mode".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"add\""));
        assert!(json.contains("\"content\":\"User prefers dark mode\""));
        let decoded: MemoryAction = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn memory_action_replace_roundtrip() {
        let action = MemoryAction::Replace {
            old_text: "dark mode".into(),
            content: "User prefers light mode in VS Code, dark in terminal".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"replace\""));
        let decoded: MemoryAction = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn memory_event_roundtrip() {
        let event = CoreEvent::MemoryAdded {
            target: MemoryTarget::Memory,
            content: "This machine runs Ubuntu 22.04".into(),
            usage: "42% — 924/2,200 chars".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"memory_added\""));
        assert!(json.contains("\"target\":\"memory\""));
    }

    #[test]
    fn memory_command_roundtrip() {
        let cmd = CoreCommand::MemoryAction {
            target: MemoryTarget::User,
            action: MemoryAction::Add {
                content: "Name: Alex".into(),
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"command\":\"memory_action\""));
        let decoded: CoreCommand = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn memory_config_default() {
        let config = MemoryConfig::default();
        assert!(config.memory_enabled);
        assert!(config.user_profile_enabled);
        assert_eq!(config.memory_char_limit, 2200);
        assert_eq!(config.user_char_limit, 1375);
        assert!(config.security_scan);
    }

    #[test]
    fn max_iterations_reached_roundtrip() {
        let event = CoreEvent::MaxIterationsReached { limit: 25 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"max_iterations_reached\""));
        assert!(json.contains("\"limit\":25"));
        let decoded: CoreEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            CoreEvent::MaxIterationsReached { limit } => assert_eq!(limit, 25),
            _ => panic!("wrong variant"),
        }
    }
}

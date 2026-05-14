use serde::{Deserialize, Serialize};

use crate::enums::StopReason;
use crate::types::TokenUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Role {
    #[serde(alias = "user")]
    User,
    #[serde(alias = "assistant")]
    Assistant,
    #[serde(alias = "tool")]
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_content: Option<String>,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
        }
    }

    pub fn assistant_tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
    ) -> ContentBlock {
        ContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self::tool_result_with_model_content(tool_use_id, content, None::<String>, is_error)
    }

    pub fn tool_result_with_model_content(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        model_content: Option<impl Into<String>>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                model_content: model_content.map(Into::into),
                is_error,
            }],
            usage: None,
            stop_reason: None,
        }
    }
}

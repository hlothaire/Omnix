use serde::{Deserialize, Serialize};

use crate::enums::{MemoryTarget, RiskLevel, StopReason};
use crate::message::ChatMessage;
use crate::types::{TokenUsage, ToolResultContent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CoreEvent {
    TokenDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    ToolCallStarted {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolCallCompleted {
        id: String,
        name: String,
        output: ToolResultContent,
    },
    TurnStarted {
        model: String,
    },
    TurnEnded {
        stop_reason: StopReason,
        usage: TokenUsage,
    },
    ApprovalRequested {
        call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        risk_level: RiskLevel,
        description: String,
    },
    SessionCreated {
        id: String,
        provider: String,
        model: String,
    },
    SessionLoaded {
        id: String,
        messages: Vec<ChatMessage>,
        total_input_tokens: u64,
        total_output_tokens: u64,
        title: String,
        provider: String,
        model: String,
    },
    SessionSaved {
        id: String,
    },
    SessionDeleted {
        id: String,
    },
    SessionCompacted {
        session_id: String,
        summary: String,
        removed_count: usize,
        messages: Vec<ChatMessage>,
        tokens_before: u64,
        input_budget: u64,
        first_kept_index: usize,
    },
    UsageUpdated {
        tokens_in: u64,
        tokens_out: u64,
        cost_usd: f64,
    },
    ContextUpdated {
        session_id: String,
        used_tokens: u64,
        max_tokens: Option<u64>,
        percent: Option<f64>,
    },
    ModelChanged {
        model: String,
    },
    ProviderStatusChanged {
        provider: String,
        connected: bool,
    },
    ToolRegistered {
        name: String,
    },
    ToolUnregistered {
        name: String,
    },
    ApiError {
        message: String,
        retryable: bool,
    },
    ToolError {
        call_id: String,
        message: String,
    },
    FatalError {
        message: String,
    },
    MemoryAdded {
        target: MemoryTarget,
        content: String,
        usage: String,
    },
    MemoryReplaced {
        target: MemoryTarget,
        old_text: String,
        new_text: String,
        usage: String,
    },
    MemoryRemoved {
        target: MemoryTarget,
        old_text: String,
        usage: String,
    },
    MemoryCapacityExceeded {
        target: MemoryTarget,
        current: usize,
        limit: usize,
    },
    MemorySecurityBlocked {
        target: MemoryTarget,
        reason: String,
    },
    MaxIterationsReached {
        limit: usize,
    },
    SessionListed {
        sessions: Vec<crate::types::SessionListEntry>,
    },
}

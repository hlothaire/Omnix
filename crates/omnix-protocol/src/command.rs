use serde::{Deserialize, Serialize};

use crate::enums::{ApprovalResponse, MemoryAction, MemoryTarget, PermissionMode, ProviderKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CoreCommand {
    SendPrompt { text: String },
    RespondToApproval {
        call_id: String,
        response: ApprovalResponse,
    },
    CancelTurn,
    NewSession,
    LoadSession { id: String },
    SaveSession,
    DeleteSession { id: String },
    ListSessions,
    ForkSession { branch_name: Option<String> },
    SetModel { model: String },
    SetProvider { provider: ProviderKind },
    SetPermissionMode { mode: PermissionMode },
    UpdateConfig { key: String, value: serde_json::Value },
    ReloadInstructions,
    EnableTool { name: String },
    DisableTool { name: String },
    McpConnect {
        server_name: String,
        program: String,
        args: Vec<String>,
    },
    McpDisconnect { server_name: String },
    MemoryAction { target: MemoryTarget, action: MemoryAction },
    Shutdown,
}

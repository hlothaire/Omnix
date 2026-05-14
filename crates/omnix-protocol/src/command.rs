use serde::{Deserialize, Serialize};

use crate::enums::{ApprovalResponse, MemoryAction, MemoryTarget, PermissionMode, ProviderKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CoreCommand {
    SendPrompt {
        #[serde(default)]
        session_id: Option<String>,
        text: String,
    },
    RespondToApproval {
        call_id: String,
        response: ApprovalResponse,
    },
    CancelTurn {
        #[serde(default)]
        session_id: Option<String>,
    },
    NewSession,
    LoadSession {
        id: String,
    },
    SaveSession,
    DeleteSession {
        id: String,
    },
    ListSessions,
    RenameSession {
        id: String,
        title: String,
    },
    SetModel {
        model: String,
    },
    SetProvider {
        provider: ProviderKind,
    },
    SetPermissionMode {
        mode: PermissionMode,
    },
    EnableTool {
        name: String,
    },
    DisableTool {
        name: String,
    },
    MemoryAction {
        target: MemoryTarget,
        action: MemoryAction,
    },
    Shutdown,
}

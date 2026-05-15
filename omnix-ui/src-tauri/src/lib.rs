use omnix_core::agent::AgentCore;
use omnix_core::config::AppConfig;
use omnix_core::provider::{AnyProvider, Provider};
use omnix_protocol::{ApprovalResponse, CoreCommand, CoreEvent, PermissionMode, ProviderKind};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

struct AppState {
    cmd_tx: mpsc::UnboundedSender<CoreCommand>,
    event_rx: Mutex<Option<mpsc::UnboundedReceiver<CoreEvent>>>,
    handle: Mutex<Option<AppHandle>>,
}

impl AppState {
    fn spawn_event_bridge(app: &tauri::App) {
        let state: State<AppState> = app.state();
        let rx = state
            .event_rx
            .lock()
            .unwrap()
            .take()
            .expect("event_rx already taken");

        let handle = app.handle().clone();

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                let payload = serde_json::to_value(&event).unwrap();
                let _ = handle.emit("core-event", payload);
            }
        });
    }
}

#[tauri::command]
fn send_prompt(state: State<AppState>, text: String, session_id: Option<String>) {
    let _ = state
        .cmd_tx
        .send(CoreCommand::SendPrompt { text, session_id });
}

#[tauri::command]
fn respond_to_approval(
    state: State<AppState>,
    session_id: String,
    call_id: String,
    response: String,
) {
    let resp = match response.as_str() {
        "allow_once" => ApprovalResponse::AllowOnce,
        "allow_for_session" => ApprovalResponse::AllowForSession,
        _ => ApprovalResponse::Deny,
    };
    let _ = state.cmd_tx.send(CoreCommand::RespondToApproval {
        session_id,
        call_id,
        response: resp,
    });
}

#[tauri::command]
fn new_session(state: State<AppState>) {
    let _ = state.cmd_tx.send(CoreCommand::NewSession);
}

#[tauri::command]
fn load_session(state: State<AppState>, id: String) {
    let _ = state.cmd_tx.send(CoreCommand::LoadSession { id });
}

#[tauri::command]
fn delete_session(state: State<AppState>, id: String) {
    let _ = state.cmd_tx.send(CoreCommand::DeleteSession { id });
}

#[tauri::command]
fn list_sessions(state: State<AppState>) {
    let _ = state.cmd_tx.send(CoreCommand::ListSessions);
}

#[tauri::command]
fn set_provider(state: State<AppState>, provider: String, host: String) {
    let kind = match provider.as_str() {
        "ollama" => ProviderKind::Ollama,
        "llama_cpp" | "llamacpp" => ProviderKind::LlamaCpp,
        _ => ProviderKind::LlamaCpp,
    };
    let _ = state.cmd_tx.send(CoreCommand::SetProvider {
        provider: kind,
        host,
    });
}

#[tauri::command]
fn set_permission_mode(state: State<AppState>, mode: String) {
    let m = match mode.as_str() {
        "readonly" => PermissionMode::ReadOnly,
        "workspace-write" => PermissionMode::WorkspaceWrite,
        "danger" => PermissionMode::DangerFullAccess,
        "prompt" => PermissionMode::Prompt,
        "allow" => PermissionMode::Allow,
        _ => PermissionMode::WorkspaceWrite,
    };
    let _ = state
        .cmd_tx
        .send(CoreCommand::SetPermissionMode { mode: m });
}

#[tauri::command]
fn cancel_turn(state: State<AppState>, session_id: Option<String>) {
    let _ = state.cmd_tx.send(CoreCommand::CancelTurn { session_id });
}

#[tauri::command]
fn save_session(state: State<AppState>) {
    let _ = state.cmd_tx.send(CoreCommand::SaveSession);
}

#[tauri::command]
fn rename_session(state: State<AppState>, id: String, title: String) {
    let _ = state.cmd_tx.send(CoreCommand::RenameSession { id, title });
}

#[tauri::command]
fn enable_tool(state: State<AppState>, name: String) {
    let _ = state.cmd_tx.send(CoreCommand::EnableTool { name });
}

#[tauri::command]
fn disable_tool(state: State<AppState>, name: String) {
    let _ = state.cmd_tx.send(CoreCommand::DisableTool { name });
}

#[tauri::command]
fn set_model(state: State<AppState>, model: String) {
    let _ = state.cmd_tx.send(CoreCommand::SetModel { model });
}

#[tauri::command]
async fn list_models(provider: String, host: String) -> Result<Vec<String>, String> {
    let (kind, default_host) = match provider.as_str() {
        "ollama" => ("ollama", "http://localhost:11434"),
        "llama_cpp" | "llamacpp" => ("llama_cpp", "http://localhost:8080"),
        other => return Err(format!("Unknown provider: {}", other)),
    };
    let host = if host.trim().is_empty() {
        default_host
    } else {
        host.trim()
    };

    let provider = AnyProvider::from_kind(kind, host, "").map_err(|e| e.to_string())?;

    provider.list_models().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn is_tiling_wm() -> bool {
    std::env::var("SWAYSOCK").is_ok()
        || std::env::var("I3SOCK").is_ok()
        || std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
        || std::env::var("DWL_SOCKET").is_ok()
        || std::env::var("RIVER_SOCKET").is_ok()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    let config = AppConfig::load().unwrap_or_default();

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut core =
        AgentCore::from_config(&config, event_tx.clone()).expect("Failed to initialize agent core");

    tokio::spawn(async move {
        core.run(cmd_rx).await;
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            cmd_tx: cmd_tx.clone(),
            event_rx: Mutex::new(Some(event_rx)),
            handle: Mutex::new(None),
        })
        .setup(move |app| {
            let state: tauri::State<AppState> = app.state();
            *state.handle.lock().unwrap() = Some(app.handle().clone());

            AppState::spawn_event_bridge(app);

            let _ = cmd_tx.send(omnix_protocol::CoreCommand::ListSessions);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_prompt,
            respond_to_approval,
            new_session,
            load_session,
            delete_session,
            list_sessions,
            set_provider,
            set_model,
            list_models,
            set_permission_mode,
            cancel_turn,
            save_session,
            rename_session,
            enable_tool,
            disable_tool,
            is_tiling_wm,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

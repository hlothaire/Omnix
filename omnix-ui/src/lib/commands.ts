import { invoke } from "@tauri-apps/api/core";

export const sendPrompt = (text: string, sessionId?: string | null) => invoke("send_prompt", { text, sessionId });
export const newSession = () => invoke("new_session");
export const loadSession = (id: string) => invoke("load_session", { id });
export const deleteSession = (id: string) => invoke("delete_session", { id });
export const listSessions = () => invoke("list_sessions");
export const cancelTurn = (sessionId?: string | null) => invoke("cancel_turn", { sessionId });
export const saveSession = () => invoke("save_session");
export const renameSession = (id: string, title: string) => invoke("rename_session", { id, title });
export const setProvider = (provider: string, host: string) => invoke("set_provider", { provider, host });
export const setModel = (model: string) => invoke("set_model", { model });
export const listModels = (provider: string, host: string) => invoke<string[]>("list_models", { provider, host });
export const setPermissionMode = (mode: string) => invoke("set_permission_mode", { mode });
export const enableTool = (name: string) => invoke("enable_tool", { name });
export const disableTool = (name: string) => invoke("disable_tool", { name });
export const respondToApproval = (sessionId: string, callId: string, response: string) =>
  invoke("respond_to_approval", { sessionId, callId, response });
export const isTilingWM = () => invoke<boolean>("is_tiling_wm");

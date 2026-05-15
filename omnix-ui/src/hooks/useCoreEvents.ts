import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CoreEvent } from "@/lib/types";
import { convertChatMessages, type DisplayMessage } from "@/lib/types";
import { useChatStore } from "@/store/chat";
import { useSessionStore } from "@/store/sessions";
import { useApprovalStore } from "@/store/approvals";
import { useSettingsStore } from "@/store/settings";
import { useTabStore } from "@/store/tabs";
import { listSessions } from "@/lib/commands";
import { routeContextUpdated, routeSessionCompacted, routeTokenDelta, routeTurnEnded, routeTurnStarted } from "@/lib/eventRouting";

export function useCoreEvents() {
  const chat = useChatStore();
  const sessions = useSessionStore();
  const approvals = useApprovalStore();
  const settings = useSettingsStore();

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    const applyToSnapshot = (sid: string, updater: (msgs: DisplayMessage[]) => DisplayMessage[]) => {
      const snap = useTabStore.getState().snapshots[sid];
      if (!snap) return;
      const updated = updater(snap.messages);
      useTabStore.getState().saveSnapshot(sid, { ...snap, messages: updated });
    };

    const start = async () => {
      try {
        unlisten = await listen<CoreEvent>("core-event", (e) => {
          if (cancelled) return;
          const event = e.payload;
          try {
            switch (event.event) {
              case "token_delta": {
                routeTokenDelta(event);
                break;
              }
              case "thinking_delta": {
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (event.session_id === activeId) {
                  chat.appendThinking(event.thinking);
                } else {
                  applyToSnapshot(event.session_id, (msgs) => {
                    const next = [...msgs];
                    const last = next[next.length - 1];
                    if (last && last.role.toLowerCase() === "assistant") {
                      next[next.length - 1] = { ...last, thinking: last.thinking + event.thinking };
                    } else {
                      next.push({ role: "Assistant", text: "", thinking: event.thinking, toolCalls: [], isError: false, time: new Date().toLocaleTimeString() });
                    }
                    return next;
                  });
                }
                break;
              }
              case "tool_call_started": {
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (event.session_id === activeId) {
                  chat.startToolCall(event.id, event.name, event.input);
                } else {
                  applyToSnapshot(event.session_id, (msgs) => {
                    const next = [...msgs];
                    const last = next[next.length - 1];
                    const toolCall = { callId: event.id, name: event.name, input: event.input, isError: false };
                    if (last && last.role.toLowerCase() === "assistant") {
                      next[next.length - 1] = { ...last, toolCalls: [...last.toolCalls, toolCall] };
                    } else {
                      next.push({ role: "Assistant", text: "", thinking: "", toolCalls: [toolCall], isError: false, time: new Date().toLocaleTimeString() });
                    }
                    return next;
                  });
                }
                break;
              }
              case "tool_call_completed": {
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (event.session_id === activeId) {
                  chat.completeToolCall(event.id, event.output.output, event.output.is_error);
                } else {
                  applyToSnapshot(event.session_id, (msgs) => msgs.map((m) => ({
                    ...m,
                    toolCalls: m.toolCalls.map((tc) =>
                      tc.callId === event.id
                        ? { ...tc, output: event.output.output, isError: event.output.is_error }
                      : tc
                    ),
                  })));
                }
                break;
              }
              case "tool_error": {
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (event.session_id === activeId) {
                  chat.toolError(event.call_id, event.message);
                } else {
                  applyToSnapshot(event.session_id, (msgs) => msgs.map((m) => ({
                    ...m,
                    toolCalls: m.toolCalls.map((tc) =>
                      tc.callId === event.call_id
                        ? { ...tc, output: event.message, isError: true }
                      : tc
                    ),
                  })));
                }
                break;
              }
              case "turn_started":
                routeTurnStarted(event);
                break;
              case "turn_ended": {
                routeTurnEnded(event);
                break;
              }
              case "approval_requested":
                approvals.addApproval({
                  sessionId: event.session_id,
                  callId: event.call_id,
                  toolName: event.tool_name,
                  toolInput: event.tool_input,
                  riskLevel: event.risk_level,
                  description: event.description,
                });
                break;
              case "session_created":
                sessions.addSession({ id: event.id, title: "New session", updatedAt: new Date().toISOString(), provider: event.provider, host: event.host, model: event.model });
                useTabStore.getState().openTab(event.id, "New session");
                chat.clear();
                settings.setProvider(event.provider);
                settings.setHost(event.host);
                settings.setModel(event.model);
                sessions.setActiveSession(event.id);
                break;
              case "session_loaded":
                chat.setMessages(convertChatMessages(event.messages));
                chat.setTokens(event.total_input_tokens, event.total_output_tokens);
                sessions.updateRuntime(event.id, { provider: event.provider, host: event.host, model: event.model });
                settings.setProvider(event.provider);
                settings.setHost(event.host);
                settings.setModel(event.model);
                if (event.title) sessions.updateTitle(event.id, event.title);
                break;
              case "session_saved":
                sessions.markSaved(event.id);
                break;
              case "session_deleted":
                sessions.removeSession(event.id);
                useTabStore.getState().removeBySession(event.id);
                if (useTabStore.getState().tabs.length === 0) {
                  chat.clear();
                  sessions.setActiveSession(null);
                }
                break;
              case "session_listed":
                sessions.setSessions(event.sessions.map(s => ({
                  id: s.id,
                  title: s.title || "Saved Chat",
                  updatedAt: s.updated_at,
                  provider: s.provider,
                  host: s.host,
                  model: s.model,
                })));
                break;
              case "session_compacted":
                routeSessionCompacted(event);
                break;
              case "api_error":
                if (!event.session_id || event.session_id === (useSessionStore.getState().activeSessionId ?? "")) {
                  chat.addErrorMessage(event.message);
                } else {
                  applyToSnapshot(event.session_id, (msgs) => [
                    ...msgs,
                    { role: "Assistant", text: `Error: ${event.message}`, thinking: "", toolCalls: [], isError: true, time: new Date().toLocaleTimeString() },
                  ]);
                }
                break;
              case "fatal_error":
                chat.addErrorMessage(`Fatal: ${event.message}`);
                break;
              case "max_iterations_reached":
                chat.addErrorMessage(`Max iterations reached (${event.limit})`);
                break;
              case "context_updated": {
                routeContextUpdated(event);
                break;
              }
              case "model_changed":
                settings.setModel(event.model);
                {
                  const activeSessionId = useSessionStore.getState().activeSessionId;
                  if (activeSessionId) sessions.updateRuntime(activeSessionId, { model: event.model });
                }
                break;
              case "provider_status_changed":
                if (event.connected) {
                  settings.setProvider(event.provider);
                  settings.setHost(event.host);
                  const activeSessionId = useSessionStore.getState().activeSessionId;
                  if (activeSessionId) sessions.updateRuntime(activeSessionId, { provider: event.provider, host: event.host });
                }
                break;
              case "memory_added":
              case "memory_replaced":
              case "memory_removed":
                chat.addSystemNote(`Memory ${event.event.replace("_", " ")}`);
                break;
              case "usage_updated":
                chat.setTokens(event.tokens_in, event.tokens_out);
                break;
            }
          } catch {
            // Ignore malformed events during streaming
          }
        });

        // Listener is ready — now fetch sessions
        try { listSessions(); } catch { /* Tauri not ready */ }
      } catch {
        // Tauri event system not ready yet
      }
    };

    start();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}

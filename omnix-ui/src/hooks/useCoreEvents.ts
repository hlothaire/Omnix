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
import { routeContextUpdated, routeSessionCompacted } from "@/lib/eventRouting";

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
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (chat.isStreamingForSession(activeId)) {
                  chat.appendToken(event.text);
                } else {
                  const sid = useChatStore.getState().streamingSessionId;
                  if (sid) applyToSnapshot(sid, (msgs) => {
                    const next = [...msgs];
                    const last = next[next.length - 1];
                    if (last && last.role.toLowerCase() === "assistant") {
                      next[next.length - 1] = { ...last, text: last.text + event.text };
                    } else {
                      next.push({ role: "Assistant", text: event.text, thinking: "", toolCalls: [], isError: false, time: new Date().toLocaleTimeString() });
                    }
                    return next;
                  });
                }
                break;
              }
              case "thinking_delta": {
                const activeId = useSessionStore.getState().activeSessionId ?? "";
                if (chat.isStreamingForSession(activeId)) {
                  chat.appendThinking(event.thinking);
                } else {
                  const sid = useChatStore.getState().streamingSessionId;
                  if (sid) applyToSnapshot(sid, (msgs) => {
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
                if (chat.isStreamingForSession(activeId)) {
                  chat.startToolCall(event.id, event.name, event.input);
                } else {
                  const sid = useChatStore.getState().streamingSessionId;
                  if (sid) applyToSnapshot(sid, (msgs) => {
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
                if (chat.isStreamingForSession(activeId)) {
                  chat.completeToolCall(event.id, event.output.output, event.output.is_error);
                } else {
                  const sid = useChatStore.getState().streamingSessionId;
                  if (sid) applyToSnapshot(sid, (msgs) => msgs.map((m) => ({
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
                if (chat.isStreamingForSession(activeId)) {
                  chat.toolError(event.call_id, event.message);
                } else {
                  const sid = useChatStore.getState().streamingSessionId;
                  if (sid) applyToSnapshot(sid, (msgs) => msgs.map((m) => ({
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
                chat.setStreamingSession(useSessionStore.getState().activeSessionId);
                break;
              case "turn_ended": {
                const streamingId = useChatStore.getState().streamingSessionId;
                if (chat.isStreamingForSession(useSessionStore.getState().activeSessionId ?? "")) {
                  chat.addTokens(event.usage.input_tokens, event.usage.output_tokens);
                } else if (streamingId) {
                  const snap = useTabStore.getState().snapshots[streamingId];
                  if (snap) {
                    useTabStore.getState().saveSnapshot(streamingId, {
                      ...snap,
                      isStreaming: false,
                      totalInputTokens: snap.totalInputTokens + event.usage.input_tokens,
                      totalOutputTokens: snap.totalOutputTokens + event.usage.output_tokens,
                    });
                  }
                }
                chat.setStreamingSession(null);
                break;
              }
              case "approval_requested":
                approvals.addApproval({
                  callId: event.call_id,
                  toolName: event.tool_name,
                  toolInput: event.tool_input,
                  riskLevel: event.risk_level,
                  description: event.description,
                });
                break;
              case "session_created":
                sessions.addSession({ id: event.id, title: "New session", updatedAt: new Date().toISOString(), provider: event.provider, model: event.model });
                useTabStore.getState().openTab(event.id, "New session");
                chat.clear();
                settings.setProvider(event.provider);
                settings.setModel(event.model);
                sessions.setActiveSession(event.id);
                break;
              case "session_loaded":
                chat.setMessages(convertChatMessages(event.messages));
                chat.setTokens(event.total_input_tokens, event.total_output_tokens);
                settings.setProvider(event.provider);
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
                  model: s.model,
                })));
                break;
              case "session_compacted":
                routeSessionCompacted(event);
                break;
              case "api_error":
                chat.addErrorMessage(event.message);
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
                break;
              case "provider_status_changed":
                if (event.connected) settings.setProvider(event.provider);
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

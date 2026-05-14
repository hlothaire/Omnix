import type { CoreEvent } from "@/lib/types";
import { convertChatMessages } from "@/lib/types";
import { useChatStore } from "@/store/chat";
import { useSessionStore } from "@/store/sessions";
import { useTabStore } from "@/store/tabs";

type SessionCompactedEvent = Extract<CoreEvent, { event: "session_compacted" }>;
type ContextUpdatedEvent = Extract<CoreEvent, { event: "context_updated" }>;

export function routeSessionCompacted(event: SessionCompactedEvent) {
  const activeId = useSessionStore.getState().activeSessionId ?? "";
  const compactedMessages = convertChatMessages(event.messages);

  if (event.session_id === activeId) {
    useChatStore.getState().setMessages(compactedMessages);
    return;
  }

  const snap = useTabStore.getState().snapshots[event.session_id];
  if (!snap) return;

  useTabStore.getState().saveSnapshot(event.session_id, {
    ...snap,
    messages: compactedMessages,
  });
}

export function routeContextUpdated(event: ContextUpdatedEvent) {
  const activeId = useSessionStore.getState().activeSessionId ?? "";

  if (event.session_id === activeId) {
    useChatStore
      .getState()
      .setContextUsage(event.used_tokens, event.max_tokens, event.percent);
    return;
  }

  const snap = useTabStore.getState().snapshots[event.session_id];
  if (!snap) return;

  useTabStore.getState().saveSnapshot(event.session_id, {
    ...snap,
    contextUsedTokens: event.used_tokens,
    contextMaxTokens: event.max_tokens,
    contextPercent: event.percent,
  });
}

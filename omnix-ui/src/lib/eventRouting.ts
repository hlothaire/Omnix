import type { CoreEvent } from "@/lib/types";
import { convertChatMessages } from "@/lib/types";
import { useChatStore } from "@/store/chat";
import { useSessionStore } from "@/store/sessions";
import { useTabStore } from "@/store/tabs";

type SessionCompactedEvent = Extract<CoreEvent, { event: "session_compacted" }>;
type ContextUpdatedEvent = Extract<CoreEvent, { event: "context_updated" }>;
type TokenDeltaEvent = Extract<CoreEvent, { event: "token_delta" }>;
type TurnStartedEvent = Extract<CoreEvent, { event: "turn_started" }>;
type TurnEndedEvent = Extract<CoreEvent, { event: "turn_ended" }>;

function applyToSnapshot(
  sessionId: string,
  updater: (msgs: import("@/lib/types").DisplayMessage[]) => import("@/lib/types").DisplayMessage[],
) {
  const snap = useTabStore.getState().snapshots[sessionId];
  if (!snap) return;
  useTabStore.getState().saveSnapshot(sessionId, {
    ...snap,
    messages: updater(snap.messages),
  });
}

export function routeTokenDelta(event: TokenDeltaEvent) {
  const activeId = useSessionStore.getState().activeSessionId ?? "";

  if (event.session_id === activeId) {
    useChatStore.getState().appendToken(event.text);
    return;
  }

  applyToSnapshot(event.session_id, (msgs) => {
    const next = [...msgs];
    const last = next[next.length - 1];
    if (last && last.role.toLowerCase() === "assistant") {
      next[next.length - 1] = { ...last, text: last.text + event.text };
    } else {
      next.push({
        role: "Assistant",
        text: event.text,
        thinking: "",
        toolCalls: [],
        isError: false,
        time: new Date().toLocaleTimeString(),
      });
    }
    return next;
  });
}

export function routeTurnStarted(event: TurnStartedEvent) {
  useChatStore.getState().setSessionStreaming(event.session_id, true);
}

export function routeTurnEnded(event: TurnEndedEvent) {
  const activeId = useSessionStore.getState().activeSessionId ?? "";

  if (event.session_id === activeId) {
    useChatStore.getState().addTokens(event.usage.input_tokens, event.usage.output_tokens);
  } else {
    const snap = useTabStore.getState().snapshots[event.session_id];
    if (snap) {
      useTabStore.getState().saveSnapshot(event.session_id, {
        ...snap,
        isStreaming: false,
        totalInputTokens: snap.totalInputTokens + event.usage.input_tokens,
        totalOutputTokens: snap.totalOutputTokens + event.usage.output_tokens,
      });
    }
  }

  useChatStore.getState().setSessionStreaming(event.session_id, false);
}

export function routeSessionCompacted(event: SessionCompactedEvent) {
  const activeId = useSessionStore.getState().activeSessionId ?? "";
  const compactedMessages = convertChatMessages(event.messages);
  const notice = {
    removedCount: event.removed_count,
    tokensBefore: event.tokens_before,
    inputBudget: event.input_budget,
    firstKeptIndex: event.first_kept_index,
    createdAt: new Date().toISOString(),
  };

  if (event.session_id === activeId) {
    useChatStore.getState().setMessages(compactedMessages);
    useChatStore.getState().addCompactionNotice(notice);
    return;
  }

  const snap = useTabStore.getState().snapshots[event.session_id];
  if (!snap) return;

  useTabStore.getState().saveSnapshot(event.session_id, {
    ...snap,
    messages: compactedMessages,
    compactionNotices: [...(snap.compactionNotices ?? []), notice].slice(-5),
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

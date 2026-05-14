import { beforeEach, describe, expect, it } from "vitest";
import type { ChatMessage, DisplayMessage } from "@/lib/types";
import { routeContextUpdated, routeSessionCompacted } from "@/lib/eventRouting";
import { useChatStore } from "@/store/chat";
import { useSessionStore } from "@/store/sessions";
import { useTabStore } from "@/store/tabs";

function displayMessage(text: string): DisplayMessage {
  return {
    role: "Assistant",
    text,
    thinking: "",
    toolCalls: [],
    isError: false,
    time: "test",
  };
}

function chatMessage(text: string): ChatMessage {
  return {
    role: "Assistant",
    content: [{ type: "text", text }],
  };
}

function saveSnapshot(sessionId: string, messages: DisplayMessage[] = []) {
  useTabStore.getState().saveSnapshot(sessionId, {
    messages,
    inputText: "",
    isStreaming: false,
    totalInputTokens: 0,
    totalOutputTokens: 0,
    contextUsedTokens: 1,
    contextMaxTokens: 10,
    contextPercent: 10,
    expandedTools: {},
    expandedThinking: {},
  });
}

beforeEach(() => {
  useChatStore.getState().clear();
  useSessionStore.setState({ sessions: [], activeSessionId: null });
  useTabStore.setState({ tabs: [], activeIndex: 0, snapshots: {} });
});

describe("event routing", () => {
  it("routes active context updates to chat state only", () => {
    useSessionStore.getState().setActiveSession("active");
    saveSnapshot("background", [displayMessage("background")]);

    routeContextUpdated({
      event: "context_updated",
      session_id: "active",
      used_tokens: 42,
      max_tokens: 100,
      percent: 42,
    });

    expect(useChatStore.getState().contextUsedTokens).toBe(42);
    expect(useChatStore.getState().contextMaxTokens).toBe(100);
    expect(useChatStore.getState().contextPercent).toBe(42);
    expect(useTabStore.getState().snapshots.background.contextUsedTokens).toBe(1);
  });

  it("routes background context updates to the matching snapshot only", () => {
    useSessionStore.getState().setActiveSession("active");
    useChatStore.getState().setContextUsage(5, 50, 10);
    saveSnapshot("background", [displayMessage("background")]);

    routeContextUpdated({
      event: "context_updated",
      session_id: "background",
      used_tokens: 75,
      max_tokens: 300,
      percent: 25,
    });

    expect(useChatStore.getState().contextUsedTokens).toBe(5);
    expect(useChatStore.getState().contextMaxTokens).toBe(50);
    expect(useChatStore.getState().contextPercent).toBe(10);
    expect(useTabStore.getState().snapshots.background.contextUsedTokens).toBe(75);
    expect(useTabStore.getState().snapshots.background.contextMaxTokens).toBe(300);
    expect(useTabStore.getState().snapshots.background.contextPercent).toBe(25);
  });

  it("routes active compaction messages to chat state only", () => {
    useSessionStore.getState().setActiveSession("active");
    useChatStore.getState().setMessages([displayMessage("old active")]);
    saveSnapshot("background", [displayMessage("old background")]);

    routeSessionCompacted({
      event: "session_compacted",
      session_id: "active",
      summary: "summary",
      removed_count: 2,
      messages: [chatMessage("compacted active")],
      tokens_before: 100,
      input_budget: 50,
      first_kept_index: 1,
    });

    expect(useChatStore.getState().messages).toHaveLength(1);
    expect(useChatStore.getState().messages[0].text).toBe("compacted active");
    expect(useTabStore.getState().snapshots.background.messages[0].text).toBe("old background");
  });

  it("routes background compaction messages to the matching snapshot only", () => {
    useSessionStore.getState().setActiveSession("active");
    useChatStore.getState().setMessages([displayMessage("old active")]);
    saveSnapshot("background", [displayMessage("old background")]);

    routeSessionCompacted({
      event: "session_compacted",
      session_id: "background",
      summary: "summary",
      removed_count: 2,
      messages: [chatMessage("compacted background")],
      tokens_before: 100,
      input_budget: 50,
      first_kept_index: 1,
    });

    expect(useChatStore.getState().messages[0].text).toBe("old active");
    expect(useTabStore.getState().snapshots.background.messages).toHaveLength(1);
    expect(useTabStore.getState().snapshots.background.messages[0].text).toBe(
      "compacted background",
    );
  });
});

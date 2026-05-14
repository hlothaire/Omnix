import { create } from "zustand";
import { CompactionNotice, DisplayMessage } from "@/lib/types";
import { sendPrompt, renameSession } from "@/lib/commands";
import { useSessionStore } from "./sessions";
import { useTabStore } from "./tabs";
import { useSettingsStore } from "./settings";

interface ChatState {
  messages: DisplayMessage[];
  compactionNotices: CompactionNotice[];
  inputText: string;
  isStreaming: boolean;
  streamingSessionId: string | null;
  streamingSessionIds: Record<string, boolean>;
  totalInputTokens: number;
  totalOutputTokens: number;
  contextUsedTokens: number;
  contextMaxTokens: number | null;
  contextPercent: number | null;
  expandedTools: Record<string, boolean>;
  expandedThinking: Record<number, boolean>;

  setInputText: (text: string) => void;
  appendToken: (text: string) => void;
  appendThinking: (thinking: string) => void;
  startToolCall: (id: string, name: string, input: Record<string, unknown>) => void;
  completeToolCall: (id: string, output: string, isError: boolean) => void;
  toolError: (id: string, message: string) => void;
  setStreaming: (v: boolean) => void;
  setStreamingSession: (id: string | null) => void;
  setSessionStreaming: (id: string, isStreaming: boolean) => void;
  isStreamingForSession: (id: string) => boolean;
  addTokens: (input: number, output: number) => void;
  addErrorMessage: (msg: string) => void;
  addSystemMessage: (msg: string) => void;
  addSystemNote: (msg: string) => void;
  setMessages: (msgs: DisplayMessage[]) => void;
  setCompactionNotices: (notices: CompactionNotice[]) => void;
  addCompactionNotice: (notice: CompactionNotice) => void;
  setTokens: (input: number, output: number) => void;
  setContextUsage: (used: number, max: number | null, percent: number | null) => void;
  toggleToolCard: (id: string) => void;
  toggleThinking: (idx: number) => void;
  clear: () => void;
  submitPrompt: () => void;
}

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  compactionNotices: [],
  inputText: "",
  isStreaming: false,
  streamingSessionId: null,
  streamingSessionIds: {},
  totalInputTokens: 0,
  totalOutputTokens: 0,
  contextUsedTokens: 0,
  contextMaxTokens: null,
  contextPercent: null,
  expandedTools: {},
  expandedThinking: {},

  setInputText: (text) => set({ inputText: text }),

  appendToken: (text) => set((s) => {
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role.toLowerCase() === "assistant") {
      msgs[msgs.length - 1] = { ...last, text: last.text + text };
    } else {
      msgs.push({ role: "Assistant", text, thinking: "", toolCalls: [], isError: false, time: new Date().toLocaleTimeString() });
    }
    return { messages: msgs };
  }),

  appendThinking: (thinking) => set((s) => {
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role.toLowerCase() === "assistant") {
      msgs[msgs.length - 1] = { ...last, thinking: last.thinking + thinking };
    } else {
      msgs.push({ role: "Assistant", text: "", thinking, toolCalls: [], isError: false, time: new Date().toLocaleTimeString() });
    }
    return { messages: msgs };
  }),

  startToolCall: (id, name, input) => set((s) => {
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role.toLowerCase() === "assistant") {
      msgs[msgs.length - 1] = {
        ...last,
        toolCalls: [...last.toolCalls, { callId: id, name, input, isError: false }],
      };
    } else {
      msgs.push({
        role: "Assistant",
        text: "",
        thinking: "",
        toolCalls: [{ callId: id, name, input, isError: false }],
        isError: false,
        time: new Date().toLocaleTimeString(),
      });
    }
    return { messages: msgs, expandedTools: { ...s.expandedTools, [id]: false } };
  }),

  completeToolCall: (id, output, isError) => set((s) => {
    const msgs = s.messages.map((m) => ({
      ...m,
      toolCalls: m.toolCalls.map((tc) =>
        tc.callId === id ? { ...tc, output, isError } : tc
      ),
    }));
    return { messages: msgs };
  }),

  toolError: (id, message) => set((s) => {
    const msgs = s.messages.map((m) => ({
      ...m,
      toolCalls: m.toolCalls.map((tc) =>
        tc.callId === id ? { ...tc, output: message, isError: true } : tc
      ),
    }));
    return { messages: msgs };
  }),

  setStreaming: (v) => set({ isStreaming: v }),
  setStreamingSession: (id) => set((s) => ({
    streamingSessionId: id,
    isStreaming: id !== null,
    streamingSessionIds: id ? { ...s.streamingSessionIds, [id]: true } : s.streamingSessionIds,
  })),
  setSessionStreaming: (id, isStreaming) => set((s) => {
    const streamingSessionIds = { ...s.streamingSessionIds };
    if (isStreaming) streamingSessionIds[id] = true;
    else delete streamingSessionIds[id];
    return {
      streamingSessionIds,
      streamingSessionId: isStreaming ? id : s.streamingSessionId === id ? null : s.streamingSessionId,
      isStreaming: isStreaming || (s.streamingSessionId !== id && s.isStreaming),
    };
  }),
  isStreamingForSession: (id) => Boolean(get().streamingSessionIds[id]),
  addTokens: (input, output) => set((s) => ({
    totalInputTokens: s.totalInputTokens + input,
    totalOutputTokens: s.totalOutputTokens + output,
  })),
  setTokens: (input, output) => set({ totalInputTokens: input, totalOutputTokens: output }),
  setContextUsage: (used, max, percent) => set({
    contextUsedTokens: used,
    contextMaxTokens: max,
    contextPercent: percent,
  }),

  addErrorMessage: (msg) => set((s) => ({
    messages: [...s.messages, {
      role: "Assistant", text: `Error: ${msg}`, thinking: "",
      toolCalls: [], isError: true, time: new Date().toLocaleTimeString(),
    }],
    isStreaming: false,
  })),

  addSystemMessage: (msg) => set((s) => ({
    messages: [...s.messages, {
      role: "Assistant", text: msg, thinking: "",
      toolCalls: [], isError: false, time: new Date().toLocaleTimeString(),
    }],
  })),

  addSystemNote: (msg) => set((s) => ({
    messages: [...s.messages, {
      role: "Tool", text: msg, thinking: "",
      toolCalls: [], isError: false, time: new Date().toLocaleTimeString(),
    }],
  })),

  setMessages: (msgs) => set({ messages: msgs }),
  setCompactionNotices: (notices) => set({ compactionNotices: notices }),
  addCompactionNotice: (notice) => set((s) => ({
    compactionNotices: [...s.compactionNotices, notice].slice(-5),
  })),
  toggleToolCard: (id) => set((s) => ({
    expandedTools: { ...s.expandedTools, [id]: !s.expandedTools[id] },
  })),
  toggleThinking: (idx) => set((s) => ({
    expandedThinking: { ...s.expandedThinking, [idx]: !s.expandedThinking[idx] },
  })),
  clear: () => set({
    messages: [], compactionNotices: [], inputText: "", isStreaming: false, streamingSessionId: null, streamingSessionIds: {},
    totalInputTokens: 0, totalOutputTokens: 0,
    contextUsedTokens: 0, contextMaxTokens: null, contextPercent: null,
  }),

  submitPrompt: () => {
    const { inputText, messages } = get();
    if (!inputText.trim()) return;

    const { provider, model } = useSettingsStore.getState();
    if (!provider || !model) {
      get().addErrorMessage("Choose a provider and model before sending a prompt.");
      return;
    }

    const maybeId = useSessionStore.getState().activeSessionId;
    const currentTitle = maybeId
      ? useSessionStore.getState().sessions.find((s) => s.id === maybeId)?.title
      : null;
    if (maybeId && currentTitle === "New session") {
      const shortTitle = inputText.trim().slice(0, 50);
      useTabStore.getState().updateTitle(maybeId, shortTitle);
      useSessionStore.getState().updateTitle(maybeId, shortTitle);
      renameSession(maybeId, shortTitle);
    }

    set({
      messages: [...messages, {
        role: "User", text: inputText, thinking: "",
        toolCalls: [], isError: false, time: new Date().toLocaleTimeString(),
      }],
      inputText: "",
      isStreaming: true,
      streamingSessionId: maybeId,
      streamingSessionIds: maybeId ? { ...get().streamingSessionIds, [maybeId]: true } : get().streamingSessionIds,
    });
    sendPrompt(inputText, maybeId);
  },
}));

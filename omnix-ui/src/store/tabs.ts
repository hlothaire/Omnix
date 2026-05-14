import { create } from "zustand";

interface TabState {
  tabs: { id: string; title: string }[];
  activeIndex: number;
  snapshots: Record<string, {
    messages: import("@/lib/types").DisplayMessage[];
    inputText: string;
    isStreaming: boolean;
    totalInputTokens: number;
    totalOutputTokens: number;
    contextUsedTokens: number;
    contextMaxTokens: number | null;
    contextPercent: number | null;
    expandedTools: Record<string, boolean>;
    expandedThinking: Record<number, boolean>;
  }>;
  openTab: (id: string, title: string) => void;
  closeTab: (index: number) => void;
  selectTab: (index: number) => void;
  updateTitle: (id: string, title: string) => void;
  removeBySession: (id: string) => void;
  saveSnapshot: (id: string, snap: TabState["snapshots"][string]) => void;
  loadSnapshot: (id: string) => TabState["snapshots"][string] | null;
}

export const useTabStore = create<TabState>((set, get) => ({
  tabs: [],
  activeIndex: 0,
  snapshots: {},

  openTab: (id, title) => set((s) => {
    const existing = s.tabs.findIndex((t) => t.id === id);
    if (existing >= 0) return { activeIndex: existing };
    return { tabs: [...s.tabs, { id, title }], activeIndex: s.tabs.length };
  }),

  closeTab: (index) => set((s) => {
    const tabs = s.tabs.filter((_, i) => i !== index);
    const activeIndex = tabs.length === 0 ? 0
      : index >= tabs.length ? tabs.length - 1 : index;
    return { tabs, activeIndex };
  }),

  selectTab: (index) => set({ activeIndex: index }),

  updateTitle: (id, title) => set((s) => ({
    tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)),
  })),

  removeBySession: (id) => set((s) => {
    const tabs = s.tabs.filter((t) => t.id !== id);
    const activeIndex = tabs.length === 0 ? 0
      : s.activeIndex >= tabs.length ? tabs.length - 1 : s.activeIndex;
    return { tabs, activeIndex };
  }),

  saveSnapshot: (id, snap) => set((s) => ({
    snapshots: { ...s.snapshots, [id]: snap },
  })),
  loadSnapshot: (id) => get().snapshots[id] ?? null,
}));

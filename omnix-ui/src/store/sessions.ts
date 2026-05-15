import { create } from "zustand";
import { SessionInfo } from "@/lib/types";

interface SessionState {
  sessions: SessionInfo[];
  activeSessionId: string | null;
  renameTarget: string | null;
  deleteTarget: string | null;
  menuTarget: string | null;

  setSessions: (s: SessionInfo[]) => void;
  addSession: (s: SessionInfo) => void;
  removeSession: (id: string) => void;
  updateTitle: (id: string, title: string) => void;
  updateRuntime: (id: string, runtime: Partial<Pick<SessionInfo, "provider" | "host" | "model">>) => void;
  markSaved: (id: string) => void;
  setActiveSession: (id: string | null) => void;
  setRenameTarget: (id: string | null) => void;
  setDeleteTarget: (id: string | null) => void;
  setMenuTarget: (id: string | null) => void;
}

export const useSessionStore = create<SessionState>((set) => ({
  sessions: [],
  activeSessionId: null,
  renameTarget: null,
  deleteTarget: null,
  menuTarget: null,

  setSessions: (sessions) => set({
    sessions: [...sessions].sort(
      (a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
    ),
  }),
  addSession: (s) => set((state) => ({
    sessions: [...state.sessions, s].sort(
      (a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
    ),
  })),
  removeSession: (id) => set((state) => ({
    sessions: state.sessions.filter((s) => s.id !== id),
    activeSessionId: state.activeSessionId === id ? null : state.activeSessionId,
  })),
  updateTitle: (id, title) => set((state) => ({
    sessions: state.sessions.map((s) => s.id === id ? { ...s, title } : s),
  })),
  updateRuntime: (id, runtime) => set((state) => ({
    sessions: state.sessions.map((s) => s.id === id ? { ...s, ...runtime } : s),
  })),
  markSaved: (id) => set((state) => {
    const sessions = state.sessions.map((s) =>
      s.id === id ? { ...s, updatedAt: new Date().toISOString() } : s
    ).sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());
    return { sessions };
  }),
  setActiveSession: (id) => set({ activeSessionId: id }),
  setRenameTarget: (id) => set({ renameTarget: id }),
  setDeleteTarget: (id) => set({ deleteTarget: id }),
  setMenuTarget: (id) => set({ menuTarget: id }),
}));

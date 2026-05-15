import { create } from "zustand";
import { PermissionMode } from "@/lib/types";

export type ThemeName = "default" | "catppuccin-macchiato" | "dracula" | "rose-pine";

interface SettingsState {
  model: string;
  provider: string;
  host: string;
  permissionMode: PermissionMode;
  theme: ThemeName;

  setModel: (model: string) => void;
  setProvider: (provider: string) => void;
  setHost: (host: string) => void;
  setPermissionMode: (mode: PermissionMode) => void;
  setTheme: (theme: ThemeName) => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  model: "",
  provider: "",
  host: "",
  permissionMode: "WorkspaceWrite",
  theme: "default",

  setModel: (model) => set({ model }),
  setProvider: (provider) => set({ provider }),
  setHost: (host) => set({ host }),
  setPermissionMode: (mode) => set({ permissionMode: mode }),
  setTheme: (theme) => {
    document.documentElement.setAttribute("data-theme", theme);
    set({ theme });
  },
}));

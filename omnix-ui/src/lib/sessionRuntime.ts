import { useSessionStore } from "@/store/sessions";
import { useSettingsStore } from "@/store/settings";

export function syncSettingsFromSession(sessionId: string) {
  const session = useSessionStore.getState().sessions.find((s) => s.id === sessionId);
  if (!session) return;

  const settings = useSettingsStore.getState();
  settings.setProvider(session.provider);
  settings.setHost(session.host);
  settings.setModel(session.model);
}

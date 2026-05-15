import { useChatStore } from "@/store/chat";
import { useSettingsStore } from "@/store/settings";
import { Activity } from "lucide-react";

export function StatusBar() {
  const {
    contextUsedTokens,
    contextMaxTokens,
    contextPercent,
    isStreaming,
    messages,
  } =
    useChatStore();
  const { model, provider, host, permissionMode } = useSettingsStore();

  if (!model && messages.length === 0) return null;

  return (
    <div className="flex items-center justify-center px-4 py-1 border-t border-border bg-secondary text-[11px] select-none">
      <div className="flex items-center gap-4 max-w-[var(--composer-w,760px)] w-full">
        {provider && (
          <span className="text-foreground">{provider}</span>
        )}
        {host && (
          <span className="max-w-40 truncate font-mono text-[10px] text-muted-foreground">
            {host}
          </span>
        )}
        {model && (
          <span className="font-medium text-foreground">{model}</span>
        )}
        {permissionMode && (
          <span>
            <span className="text-muted-foreground">Mode</span>{" "}
            <span className="text-primary font-mono text-[10px]">
              {permissionMode}
            </span>
          </span>
        )}

        <span className="flex-1" />

        {isStreaming && (
          <span className="flex items-center gap-1.5 text-primary">
            <Activity className="size-3 animate-pulse-soft" />
            <span className="animate-pulse-soft">Streaming</span>
          </span>
        )}
        <span className="text-muted-foreground">
          Context{" "}
          <span className="text-foreground font-mono text-[10px]">
            {formatTokens(contextUsedTokens)}
          </span>
          <span className="opacity-25 mx-1">/</span>
          <span className="text-foreground font-mono text-[10px]">
            {contextMaxTokens ? formatTokens(contextMaxTokens) : "unknown"}
          </span>
          {contextPercent !== null && (
            <span className="opacity-50"> ({contextPercent.toFixed(1)}%)</span>
          )}
        </span>
      </div>
    </div>
  );
}

function formatTokens(value: number) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}m`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return value.toString();
}

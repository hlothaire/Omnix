import { useTabStore } from "@/store/tabs";
import { useChatStore } from "@/store/chat";
import { useSessionStore } from "@/store/sessions";
import { cn } from "@/lib/utils";
import { X } from "lucide-react";

export function TabBar() {
  const {
    tabs,
    activeIndex,
    selectTab,
    closeTab,
    saveSnapshot,
    loadSnapshot,
  } = useTabStore();
  const chat = useChatStore();
  const { setActiveSession } = useSessionStore();

  if (tabs.length === 0) return null;

  const handleSelect = (index: number) => {
    const currentTab = tabs[activeIndex];
    if (currentTab) {
      saveSnapshot(currentTab.id, {
        messages: chat.messages,
        compactionNotices: chat.compactionNotices,
        inputText: chat.inputText,
        isStreaming: chat.isStreaming,
        totalInputTokens: chat.totalInputTokens,
        totalOutputTokens: chat.totalOutputTokens,
        contextUsedTokens: chat.contextUsedTokens,
        contextMaxTokens: chat.contextMaxTokens,
        contextPercent: chat.contextPercent,
        expandedTools: chat.expandedTools,
        expandedThinking: chat.expandedThinking,
      });
    }

    selectTab(index);
    setActiveSession(tabs[index].id);

    const snap = loadSnapshot(tabs[index].id);
    if (snap) {
      chat.setMessages(snap.messages);
      chat.setCompactionNotices(snap.compactionNotices ?? []);
      chat.setInputText(snap.inputText);
      chat.setStreaming(
        snap.isStreaming && useChatStore.getState().isStreamingForSession(tabs[index].id)
      );
      chat.setTokens(snap.totalInputTokens, snap.totalOutputTokens);
      chat.setContextUsage(snap.contextUsedTokens, snap.contextMaxTokens, snap.contextPercent);
    }
  };

  const handleClose = (index: number, e: React.MouseEvent) => {
    e.stopPropagation();
    closeTab(index);

    const remaining = useTabStore.getState().tabs;
    if (remaining.length === 0) {
      chat.clear();
      setActiveSession(null);
    } else {
      const nextTab = remaining[Math.min(useTabStore.getState().activeIndex, remaining.length - 1)];
      if (nextTab) {
        const snap = loadSnapshot(nextTab.id);
        if (snap) {
          chat.setMessages(snap.messages);
          chat.setCompactionNotices(snap.compactionNotices ?? []);
          chat.setInputText(snap.inputText);
          chat.setStreaming(
            snap.isStreaming && useChatStore.getState().isStreamingForSession(nextTab.id)
          );
          chat.setTokens(snap.totalInputTokens, snap.totalOutputTokens);
          chat.setContextUsage(snap.contextUsedTokens, snap.contextMaxTokens, snap.contextPercent);
        }
        setActiveSession(nextTab.id);
      }
    }
  };

  return (
    <div className="flex items-center gap-0.5 px-2 pt-1.5 pb-0 bg-secondary overflow-x-auto">
      {tabs.map((tab, i) => (
        <div
          key={tab.id}
          onClick={() => handleSelect(i)}
          className={cn(
            "group relative flex items-center gap-1.5 px-3.5 py-1.5 text-xs cursor-pointer select-none rounded-t-lg transition-all duration-150",
            i === activeIndex
              ? "bg-background text-foreground"
              : "text-muted-foreground hover:text-foreground hover:bg-accent/40"
          )}
        >
          <span className="max-w-[160px] truncate">{tab.title}</span>
          <div
            className="flex items-center justify-center rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
            onClick={(e) => handleClose(i, e)}
          >
            <X className="size-3 p-0.5 rounded-full hover:bg-accent/50" />
          </div>
        </div>
      ))}
    </div>
  );
}

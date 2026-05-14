import { useSessionStore } from "@/store/sessions";
import { useTabStore } from "@/store/tabs";
import { useChatStore } from "@/store/chat";
import {
  newSession,
  loadSession,
  deleteSession,
  renameSession,
} from "@/lib/commands";
import {
  SidebarHeader,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
  useSidebar,
} from "@/components/ui/sidebar";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ProviderMenu } from "./ProviderMenu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Plus,
  MoreHorizontal,
  Trash2,
  Pencil,
  PanelLeftClose,
  PanelLeft,
} from "lucide-react";

export function AppSidebar() {
  const {
    sessions,
    activeSessionId,
    renameTarget,
    menuTarget,
    setActiveSession,
    setRenameTarget,
    setMenuTarget,
    updateTitle: updateSessionTitle,
  } = useSessionStore();
  const {
    openTab,
    saveSnapshot,
    loadSnapshot,
    removeBySession,
    updateTitle: updateTabTitle,
  } = useTabStore();
  const chat = useChatStore();
  const { state, toggleSidebar } = useSidebar();
  const isCollapsed = state === "collapsed";

  const handleSelectSession = (id: string) => {
    const currentTab = useTabStore
      .getState()
      .tabs.find((t) => t.id === activeSessionId);
    if (currentTab) {
      saveSnapshot(currentTab.id, {
        messages: chat.messages,
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

    const snap = loadSnapshot(id);
    if (snap) {
      chat.setMessages(snap.messages);
      chat.setInputText(snap.inputText);
      chat.setStreaming(
        snap.isStreaming && useChatStore.getState().isStreamingForSession(id)
      );
      chat.setTokens(snap.totalInputTokens, snap.totalOutputTokens);
      chat.setContextUsage(snap.contextUsedTokens, snap.contextMaxTokens, snap.contextPercent);
    } else {
      chat.clear();
      loadSession(id);
    }

    const title = sessions.find((s) => s.id === id)?.title || "Chat";
    openTab(id, title);
    setActiveSession(id);
  };

  const handleDelete = (id: string) => {
    deleteSession(id);
    removeBySession(id);

    const remaining = useTabStore.getState().tabs.filter((t) => t.id !== id);
    if (remaining.length === 0) {
      chat.clear();
      setActiveSession(null);
    } else if (activeSessionId === id) {
      const nextTab =
        remaining[
          Math.min(useTabStore.getState().activeIndex, remaining.length - 1)
        ];
      if (nextTab) {
        const snap = loadSnapshot(nextTab.id);
        if (snap) {
          chat.setMessages(snap.messages);
          chat.setInputText(snap.inputText);
          chat.setStreaming(
            snap.isStreaming && useChatStore.getState().isStreamingForSession(nextTab.id)
          );
          chat.setTokens(snap.totalInputTokens, snap.totalOutputTokens);
          chat.setContextUsage(snap.contextUsedTokens, snap.contextMaxTokens, snap.contextPercent);
        } else {
          chat.clear();
          loadSession(nextTab.id);
        }
        setActiveSession(nextTab.id);
      }
    }
  };

  const handleRename = (id: string, title: string) => {
    if (title.trim()) {
      renameSession(id, title.trim());
      updateSessionTitle(id, title.trim());
      updateTabTitle(id, title.trim());
    }
    setRenameTarget(null);
  };

  if (isCollapsed) {
    return (
      <div className="w-12 flex-shrink-0 flex flex-col items-center pt-3 border-r border-sidebar-border bg-sidebar">
        <Button variant="ghost" size="icon" onClick={toggleSidebar}>
          <PanelLeft data-icon />
        </Button>
        <div className="mt-2">
          <ProviderMenu />
        </div>
        <div className="mt-2">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => newSession()}
            title="New session"
          >
            <Plus data-icon />
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="w-60 flex-shrink-0 flex flex-col border-r border-sidebar-border bg-sidebar">
      <SidebarHeader className="flex-row items-center justify-between py-2">
        <span className="text-sm font-semibold text-sidebar-foreground tracking-tight">
          Omnix
        </span>
        <Button variant="ghost" size="icon-sm" onClick={toggleSidebar}>
          <PanelLeftClose data-icon />
        </Button>
      </SidebarHeader>
      <SidebarSeparator />

      <SidebarContent>
        <SidebarGroup className="p-1 pt-4">
          <div className="mb-1">
            <ProviderMenu />
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => newSession()}
            className="w-full justify-start mb-0.5 text-muted-foreground hover:text-foreground"
          >
            <Plus data-icon />
            New session
          </Button>

          <div className="px-2 text-xs font-medium text-sidebar-foreground/50 mt-4 mb-2.5">
            Recent
          </div>
          <SidebarMenu className="gap-1">
            {sessions.length === 0 && (
              <div className="px-3 py-8 text-center text-xs text-muted-foreground">
                No sessions yet
              </div>
            )}
            {sessions.map((s) => (
              <SidebarMenuItem key={s.id}>
                {renameTarget === s.id ? (
                  <div className="px-2">
                    <Input
                      autoFocus
                      defaultValue={s.title}
                      className="h-6 text-xs"
                      onKeyDown={(e) => {
                        if (e.key === "Enter")
                          handleRename(s.id, e.currentTarget.value);
                        if (e.key === "Escape") setRenameTarget(null);
                      }}
                      onBlur={(e) => handleRename(s.id, e.currentTarget.value)}
                      onClick={(e) => e.stopPropagation()}
                    />
                  </div>
                ) : (
                  <>
                    <SidebarMenuButton
                      onClick={() => handleSelectSession(s.id)}
                      isActive={activeSessionId === s.id}
                      size="sm"
                      className="h-6.5 px-2 py-0.5 text-[11px]"
                    >
                      {s.title}
                    </SidebarMenuButton>
                    <DropdownMenu
                      open={menuTarget === s.id}
                      onOpenChange={(open) => setMenuTarget(open ? s.id : null)}
                    >
                      <DropdownMenuTrigger className="absolute top-1.5 right-1 flex aspect-square w-5 items-center justify-center rounded-md text-sidebar-foreground opacity-0 group-hover/menu-item:opacity-100 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground transition-opacity [&>svg]:size-3.5 [&>svg]:shrink-0">
                        <MoreHorizontal data-icon />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-40">
                        <DropdownMenuGroup>
                          <DropdownMenuItem
                            onClick={(e) => {
                              e.stopPropagation();
                              setRenameTarget(s.id);
                              setMenuTarget(null);
                            }}
                          >
                            <Pencil data-icon />
                            Rename
                          </DropdownMenuItem>
                        </DropdownMenuGroup>
                        <DropdownMenuGroup>
                          <DropdownMenuItem
                            variant="destructive"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleDelete(s.id);
                              setMenuTarget(null);
                            }}
                          >
                            <Trash2 data-icon />
                            Delete
                          </DropdownMenuItem>
                        </DropdownMenuGroup>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </>
                )}
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>

      <SidebarSeparator />
      <SidebarFooter>
        <p className="px-2 text-[10px] text-muted-foreground">
          Ctrl+B to toggle
        </p>
      </SidebarFooter>
    </div>
  );
}

import { useEffect } from "react";
import { useSessionStore } from "@/store/sessions";
import { newSession } from "@/lib/commands";
import { PromptInput } from "./PromptInput";
import { MessageList } from "./MessageList";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";

function WelcomeScreen() {
  const handleNewSession = () => newSession();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "n") {
        e.preventDefault();
        handleNewSession();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <div className="h-full flex items-center justify-center relative">
      <div className="relative z-10 text-center px-8 max-w-md animate-slide-up">
        <div className="mb-8">
          <div className="relative inline-block">
            <div className="absolute -inset-10 bg-primary/5 rounded-full blur-3xl animate-subtle-glow" />
            <h1 className="relative text-5xl font-bold tracking-tight text-foreground">
              Omnix
            </h1>
          </div>

          <div className="mt-3 flex items-center justify-center gap-3">
            <div className="h-px w-10 bg-border" />
            <span className="text-[11px] font-mono tracking-[0.28em] uppercase text-muted-foreground">
              AI Agent Harness
            </span>
            <div className="h-px w-10 bg-border" />
          </div>
        </div>

        <p className="text-sm text-muted-foreground mb-8 leading-relaxed max-w-xs mx-auto">
          Orchestrate autonomous agents with natural language.
          Tools, memory, and multi-session control.
        </p>

        <Button
          variant="outline"
          size="sm"
          onClick={handleNewSession}
          className="mb-6"
        >
          <Plus data-icon />
          New session
          <kbd className="ml-2 px-1.5 py-0.5 rounded text-[10px] font-mono bg-secondary border border-border text-muted-foreground">
            Ctrl+N
          </kbd>
        </Button>

        <div className="flex flex-col gap-2 text-xs text-muted-foreground">
          <div className="flex items-center justify-center gap-2">
            <kbd className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-secondary border border-border text-foreground">
              Enter
            </kbd>
            <span>Send</span>
            <span className="opacity-30">·</span>
            <kbd className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-secondary border border-border text-foreground">
              Shift + Enter
            </kbd>
            <span>New line</span>
          </div>
        </div>
      </div>
    </div>
  );
}

export function ChatArea() {
  const { activeSessionId } = useSessionStore();
  const hasSession = activeSessionId !== null;

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="flex-1 min-h-0">
        {!hasSession ? (
          <WelcomeScreen />
        ) : (
          <MessageList />
        )}
      </div>
      {hasSession && <PromptInput />}
    </div>
  );
}

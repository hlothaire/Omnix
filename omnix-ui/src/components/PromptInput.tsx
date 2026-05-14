import { useRef, KeyboardEvent } from "react";
import { useChatStore } from "@/store/chat";
import { cancelTurn } from "@/lib/commands";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Send, Square, Sparkles } from "lucide-react";

export function PromptInput() {
  const { inputText, setInputText, submitPrompt, isStreaming } = useChatStore();
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleSend = () => {
    if (isStreaming) {
      cancelTurn();
      return;
    }
    submitPrompt();
    setTimeout(() => {
      textareaRef.current?.focus();
    }, 0);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const hasText = inputText.trim().length > 0;

  return (
    <div className="px-4 pb-4 pt-2">
      <div className="max-w-[var(--composer-w,760px)] mx-auto">
        <div className="relative rounded-[var(--composer-radius)] bg-[var(--composer)] ring-1 ring-border shadow-[var(--composer-shadow)] transition-all duration-200 focus-within:ring-primary/20 focus-within:shadow-[0_12px_48px_rgba(167,139,250,0.08)]">
          <div className="flex items-end gap-0 p-1.5">
            <div className="flex items-center justify-center size-9 flex-shrink-0 text-muted-foreground">
              <Sparkles className="size-4" />
            </div>

            <textarea
              ref={textareaRef}
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Tell the agent what to do..."
              rows={1}
              className="flex-1 resize-none bg-transparent px-0 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
              style={{ minHeight: "36px", maxHeight: "120px" }}
              onInput={(e) => {
                const el = e.currentTarget;
                el.style.height = "auto";
                el.style.height = Math.min(el.scrollHeight, 120) + "px";
              }}
            />

            <Button
              size="icon"
              variant="ghost"
              onClick={handleSend}
              disabled={!isStreaming && !hasText}
              className={cn(
                "size-9 rounded-xl flex-shrink-0",
                isStreaming && "bg-destructive text-destructive-foreground hover:bg-destructive/90",
                !isStreaming && hasText && "bg-accent/15 text-accent hover:bg-accent/25",
                !isStreaming && !hasText && "text-muted-foreground"
              )}
            >
              {isStreaming ? (
                <Square data-icon />
              ) : (
                <Send data-icon />
              )}
            </Button>
          </div>
        </div>

        <p className="text-center text-[10px] text-muted-foreground mt-2 opacity-50">
          Enter to send · Shift+Enter for new line
        </p>
      </div>
    </div>
  );
}

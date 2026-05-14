import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import { cn } from "@/lib/utils";
import type { DisplayMessage } from "@/lib/types";
import { ToolCard } from "./ToolCard";
import { ThinkingBlock } from "./ThinkingBlock";

interface Props {
  message: DisplayMessage;
  index: number;
}

export function MessageBubble({ message, index }: Props) {
  const isUser = message.role.toLowerCase() === "user";
  const isTool = message.role.toLowerCase() === "tool";

  if (isTool) {
    return (
      <div
        className="px-4 py-1 animate-message-appear"
        style={{ animationDelay: `${Math.min(index * 25, 250)}ms` }}
      >
        <p className="text-xs text-muted-foreground italic">
          {message.text}
        </p>
      </div>
    );
  }

  return (
    <div
      className={cn(
        "flex px-5 py-4 gap-4 animate-message-appear",
        isUser ? "flex-row-reverse" : "flex-row"
      )}
      style={{ animationDelay: `${Math.min(index * 35, 350)}ms` }}
    >
      <div
        className={cn(
          "size-8 rounded-xl flex items-center justify-center flex-shrink-0 text-[11px] font-semibold select-none ring-1",
          isUser
            ? "bg-primary/10 text-primary ring-primary/20"
            : "bg-secondary text-muted-foreground ring-border"
        )}
      >
        {isUser ? "You" : "Ox"}
      </div>

      <div
        className={cn(
          "flex-1 min-w-0 max-w-[80%]",
          isUser && "flex flex-col items-end"
        )}
      >
        {message.thinking && (
          <ThinkingBlock thinking={message.thinking} index={index} />
        )}

        {message.toolCalls.length > 0 && (
          <div className="mb-3 flex flex-col gap-2 rounded-2xl border border-border/70 bg-secondary/35 p-2">
            <div className="flex items-center gap-2 px-1 text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
              <span className="h-px flex-1 bg-border" />
              Agent actions
              <span className="h-px flex-1 bg-border" />
            </div>
            {message.toolCalls.map((tc) => (
              <ToolCard key={tc.callId} tool={tc} />
            ))}
          </div>
        )}

        {message.text && (
          <div
            className={cn(
              message.isError
                ? "text-destructive text-sm"
                : isUser
                  ? "bg-primary/10 text-foreground px-4 py-2.5 rounded-2xl rounded-tr-sm text-sm leading-relaxed"
                  : "chat-markdown"
            )}
          >
            {isUser ? (
              message.text
            ) : (
              <ReactMarkdown rehypePlugins={[rehypeHighlight]}>
                {message.text}
              </ReactMarkdown>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

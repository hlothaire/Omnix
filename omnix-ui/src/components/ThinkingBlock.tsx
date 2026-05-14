import { useChatStore } from "@/store/chat";
import { cn } from "@/lib/utils";
import { ChevronRight } from "lucide-react";

interface Props {
  thinking: string;
  index: number;
}

export function ThinkingBlock({ thinking, index }: Props) {
  const { expandedThinking, toggleThinking } = useChatStore();
  const isExpanded = expandedThinking[index] ?? false;

  return (
    <div className="mb-2">
      <button
        onClick={() => toggleThinking(index)}
        className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        <ChevronRight
          className={cn(
            "size-3 transition-transform",
            isExpanded && "rotate-90"
          )}
        />
        <span className="font-medium">Thinking</span>
      </button>
      {isExpanded && (
        <div className="mt-1.5 pl-5 text-xs text-muted-foreground italic whitespace-pre-wrap leading-relaxed border-l-2 border-primary/30">
          {thinking}
        </div>
      )}
    </div>
  );
}

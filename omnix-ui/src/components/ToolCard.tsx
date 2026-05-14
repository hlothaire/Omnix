import { useChatStore } from "@/store/chat";
import type { ToolCallDisplay } from "@/lib/types";
import { cn } from "@/lib/utils";
import {
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  FileCode2,
  FilePenLine,
  FileText,
  Search,
  Terminal,
} from "lucide-react";

interface Props {
  tool: ToolCallDisplay;
}

export function ToolCard({ tool }: Props) {
  const { expandedTools, toggleToolCard } = useChatStore();
  const isExpanded = expandedTools[tool.callId] ?? false;
  const isRunning = tool.output === undefined;

  const toolMeta = getToolMeta(tool.name);

  const formatInput = (input: Record<string, unknown>) => {
    if (input.command) return input.command as string;
    if (input.path) return input.path as string;
    if (input.pattern) return input.pattern as string;
    return JSON.stringify(input, null, 2);
  };

  const preview = formatInput(tool.input);
  const StatusIcon = isRunning ? null : tool.isError ? CircleAlert : CheckCircle2;

  return (
    <div
      className={cn(
        "overflow-hidden rounded-xl border bg-card/70 shadow-sm transition-colors",
        isRunning && "border-primary/25 bg-primary/5",
        tool.isError && "border-destructive/30 bg-destructive/5"
      )}
    >
      <button
        onClick={() => toggleToolCard(tool.callId)}
        className="w-full flex items-center gap-2.5 px-3 py-2.5 text-xs text-left hover:bg-accent/30 transition-colors"
      >
        <ChevronRight
          className={cn(
            "size-3.5 transition-transform text-muted-foreground flex-shrink-0",
            isExpanded && "rotate-90"
          )}
        />

        <span className="flex size-7 flex-shrink-0 items-center justify-center rounded-lg bg-secondary text-muted-foreground ring-1 ring-border">
          <toolMeta.icon className="size-3.5" />
        </span>

        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2">
            <span className="font-medium text-foreground">
              {toolMeta.label}
            </span>
            <span
              className={cn(
                "rounded-full px-1.5 py-0.5 text-[10px] font-medium",
                isRunning && "bg-primary/10 text-primary",
                !isRunning && !tool.isError && "bg-secondary text-muted-foreground",
                tool.isError && "bg-destructive/10 text-destructive"
              )}
            >
              {isRunning ? "Running" : tool.isError ? "Failed" : "Done"}
            </span>
          </span>
          <span className="mt-0.5 block truncate font-mono text-[10px] text-muted-foreground">
            {preview}
          </span>
        </span>

        {StatusIcon ? (
          <StatusIcon
            className={cn(
              "size-4 flex-shrink-0",
              tool.isError ? "text-destructive" : "text-muted-foreground"
            )}
          />
        ) : (
          <span className="typing-dots flex-shrink-0">
            <span /><span /><span />
          </span>
        )}
      </button>

      {isExpanded && (
        <div className="border-t border-border bg-background/35">
          <section>
            <div className="px-3 py-1.5 text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
              Input
            </div>
            <pre className="px-3 pb-3 text-xs text-foreground font-mono overflow-x-auto max-h-40 overflow-y-auto whitespace-pre-wrap leading-relaxed">
              {JSON.stringify(tool.input, null, 2)}
            </pre>
          </section>

          {tool.output !== undefined && (
            <section>
              <div className="px-3 py-1.5 text-[10px] font-medium text-muted-foreground uppercase tracking-wider border-t border-border">
                Output
              </div>
              <pre
                className={cn(
                  "px-3 pb-3 text-xs font-mono overflow-x-auto max-h-60 overflow-y-auto whitespace-pre-wrap leading-relaxed",
                  tool.isError ? "text-destructive" : "text-foreground"
                )}
              >
                {tool.output}
              </pre>
            </section>
          )}

          {tool.output === undefined && (
            <div className="px-3 py-4 text-xs text-muted-foreground text-center">
              Waiting for tool output
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function getToolMeta(name: string) {
  const normalized = name.toLowerCase();

  if (normalized.includes("bash")) {
    return { label: "Run command", icon: Terminal };
  }
  if (normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")) {
    return { label: "Search workspace", icon: Search };
  }
  if (normalized.includes("edit") || normalized.includes("write")) {
    return { label: "Modify file", icon: FilePenLine };
  }
  if (normalized.includes("read")) {
    return { label: "Read file", icon: FileText };
  }

  return {
    label: name
      .split("_")
      .filter(Boolean)
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ") || "Use tool",
    icon: FileCode2,
  };
}

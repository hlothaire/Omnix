import { useEffect, useState } from "react";
import { Cpu, RefreshCw, Server, Settings2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { listModels, setModel, setProvider } from "@/lib/commands";
import { useChatStore } from "@/store/chat";
import { useSettingsStore } from "@/store/settings";
import { cn } from "@/lib/utils";

const providers = [
  {
    id: "llama_cpp",
    label: "llama.cpp",
    description: "OpenAI-compatible local server on localhost:8080",
    icon: Cpu,
  },
  {
    id: "ollama",
    label: "Ollama",
    description: "Local Ollama runtime on localhost:11434",
    icon: Server,
  },
];

export function ProviderMenu() {
  const { provider, model } = useSettingsStore();
  const { messages, streamingSessionIds } = useChatStore();
  const [open, setOpen] = useState(false);
  const [draftProvider, setDraftProvider] = useState(provider);
  const [draftModel, setDraftModel] = useState(model);
  const [models, setModels] = useState<string[]>([]);
  const [isLoadingModels, setIsLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const hasStreamingSession = Object.keys(streamingSessionIds).length > 0;
  const isLocked = messages.length > 0 || hasStreamingSession;

  useEffect(() => {
    if (!open) return;
    setDraftProvider(provider);
    setDraftModel(model);
  }, [model, open, provider]);

  useEffect(() => {
    if (!open || !draftProvider || isLocked) return;
    let cancelled = false;

    const load = async () => {
      setIsLoadingModels(true);
      setModelError(null);
      try {
        const availableModels = await listModels(draftProvider);
        if (cancelled) return;
        setModels(availableModels);
        if (!availableModels.includes(draftModel)) {
          setDraftModel(availableModels[0] ?? "");
        }
      } catch (error) {
        if (cancelled) return;
        setModels([]);
        setDraftModel("");
        setModelError(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) setIsLoadingModels(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [draftProvider, isLocked, open]);

  const activeProvider = providers.find((p) => p.id === provider);
  const canApply = draftProvider.trim().length > 0 && draftModel.trim().length > 0 && !isLocked;

  const handleApply = () => {
    if (!canApply) return;
    if (draftProvider !== provider) setProvider(draftProvider);
    if (draftModel !== model) setModel(draftModel.trim());
    setOpen(false);
  };

  const handleRefreshModels = async () => {
    if (!draftProvider || isLocked) return;
    setIsLoadingModels(true);
    setModelError(null);
    try {
      const availableModels = await listModels(draftProvider);
      setModels(availableModels);
      if (!availableModels.includes(draftModel)) {
        setDraftModel(availableModels[0] ?? "");
      }
    } catch (error) {
      setModels([]);
      setDraftModel("");
      setModelError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsLoadingModels(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button variant="ghost" size="sm" className="h-7 w-full justify-start px-2 text-[11px] text-muted-foreground" />}>
        <Settings2 data-icon />
        <span className="truncate">
          {activeProvider?.label ?? "Provider"}
          {model ? ` · ${model}` : ""}
        </span>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Provider settings</DialogTitle>
          <DialogDescription>
            Choose the runtime and model for new agent sessions.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          {isLocked && (
            <div className="rounded-lg border border-border bg-secondary/60 px-3 py-2 text-xs text-muted-foreground">
              {hasStreamingSession
                ? "Provider and model are locked while any session is streaming."
                : "This session already has messages, so its provider and model are locked. Create a new session to use a different runtime."}
            </div>
          )}

          <div className="flex flex-col gap-2">
            <div className="text-xs font-medium text-muted-foreground">Provider</div>
            <div className="grid gap-2 sm:grid-cols-2">
              {providers.map((item) => {
                const Icon = item.icon;
                const selected = draftProvider === item.id;

                return (
                  <button
                    key={item.id}
                    type="button"
                    disabled={isLocked}
                    onClick={() => {
                      setDraftProvider(item.id);
                      setDraftModel("");
                    }}
                    className={cn(
                      "flex min-h-24 flex-col items-start gap-3 rounded-xl border bg-card p-3 text-left transition-colors hover:bg-accent/30 disabled:pointer-events-none disabled:opacity-60",
                      selected && "border-primary/40 bg-primary/10"
                    )}
                  >
                    <span className="flex size-8 items-center justify-center rounded-lg bg-secondary text-muted-foreground ring-1 ring-border">
                      <Icon className="size-4" />
                    </span>
                    <span>
                      <span className="block text-sm font-medium text-foreground">
                        {item.label}
                      </span>
                      <span className="mt-1 block text-xs leading-relaxed text-muted-foreground">
                        {item.description}
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2">
              <div className="text-xs font-medium text-muted-foreground">Model</div>
              <Button
                variant="ghost"
                size="xs"
                onClick={handleRefreshModels}
                disabled={isLocked || !draftProvider || isLoadingModels}
              >
                <RefreshCw data-icon="inline-start" className={cn(isLoadingModels && "animate-spin")} />
                Refresh
              </Button>
            </div>

            {isLoadingModels ? (
              <div className="rounded-xl border border-border bg-secondary/40 px-3 py-6 text-center text-xs text-muted-foreground">
                Loading models from provider...
              </div>
            ) : modelError ? (
              <div className="rounded-xl border border-destructive/30 bg-destructive/5 px-3 py-3 text-xs text-destructive">
                Could not load models: {modelError}
              </div>
            ) : models.length > 0 ? (
              <div className="max-h-52 overflow-y-auto rounded-xl border border-border bg-card p-1">
                {models.map((item) => (
                  <button
                    key={item}
                    type="button"
                    disabled={isLocked}
                    onClick={() => setDraftModel(item)}
                    className={cn(
                      "flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left text-xs transition-colors hover:bg-accent/30 disabled:pointer-events-none disabled:opacity-60",
                      draftModel === item && "bg-primary/10 text-primary"
                    )}
                  >
                    <span className="truncate font-mono">{item}</span>
                    {draftModel === item && <span className="text-[10px] font-medium">Selected</span>}
                  </button>
                ))}
              </div>
            ) : (
              <div className="rounded-xl border border-border bg-secondary/40 px-3 py-6 text-center text-xs text-muted-foreground">
                No models exposed by this provider.
              </div>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" size="sm" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button size="sm" onClick={handleApply} disabled={!canApply}>
            Apply
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

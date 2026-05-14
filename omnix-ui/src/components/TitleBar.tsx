import { useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/button";
import { Minus, X, Square, Copy } from "lucide-react";
import { isTilingWM } from "@/lib/commands";

export function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false);
  const [showTitleBar, setShowTitleBar] = useState(true);

  useEffect(() => {
    isTilingWM().then((isTiling) => setShowTitleBar(!isTiling)).catch(() => {});

    getCurrentWindow().isMaximized().then(setIsMaximized);
    const unlisten = getCurrentWindow().onResized(() => {
      getCurrentWindow().isMaximized().then(setIsMaximized);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  if (!showTitleBar) return null;

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-8 flex-shrink-0 bg-sidebar border-b border-border select-none"
    >
      <div className="flex items-center gap-2 pl-3">
        <span className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Omnix
        </span>
      </div>

      <div className="flex items-center h-full">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => getCurrentWindow().minimize()}
          className="h-full w-10 rounded-none text-muted-foreground hover:text-foreground hover:bg-accent"
        >
          <Minus data-icon />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => {
            getCurrentWindow().toggleMaximize();
            setIsMaximized(!isMaximized);
          }}
          className="h-full w-10 rounded-none text-muted-foreground hover:text-foreground hover:bg-accent"
        >
          {isMaximized ? <Copy data-icon /> : <Square data-icon />}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => getCurrentWindow().close()}
          className="h-full w-10 rounded-none text-muted-foreground hover:text-destructive-foreground hover:bg-destructive"
        >
          <X data-icon />
        </Button>
      </div>
    </div>
  );
}

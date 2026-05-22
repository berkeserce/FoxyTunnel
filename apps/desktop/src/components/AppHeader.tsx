import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button, Chip } from "@heroui/react";
import { Settings, X } from "lucide-react";
import { useCallback, useRef } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { statusMeta } from "../statusMeta";
import type { ProxyStatus, ViewMode } from "../types";

const DRAG_THRESHOLD_PX = 4;

type AppHeaderProps = {
  status: ProxyStatus;
  view: ViewMode;
  onOpenSettings: () => void;
  onClose: () => void;
};

export function AppHeader({ status, view, onOpenSettings, onClose }: AppHeaderProps) {
  const meta = statusMeta[status];
  const StatusIcon = meta.icon;
  const dragStart = useRef<{ x: number; y: number } | null>(null);

  const clearDragListeners = useCallback(() => {
    window.removeEventListener("mousemove", handleDragMove);
    window.removeEventListener("mouseup", clearDragListeners);
    dragStart.current = null;
  }, []);

  const handleDragMove = useCallback(
    (event: MouseEvent) => {
      const start = dragStart.current;

      if (!start) {
        return;
      }

      const movedX = Math.abs(event.screenX - start.x);
      const movedY = Math.abs(event.screenY - start.y);

      if (movedX < DRAG_THRESHOLD_PX && movedY < DRAG_THRESHOLD_PX) {
        return;
      }

      clearDragListeners();
      getCurrentWindow().startDragging().catch(() => {});
    },
    [clearDragListeners],
  );

  const prepareWindowDrag = useCallback(
    (event: ReactMouseEvent<HTMLElement>) => {
      if (event.button !== 0) {
        return;
      }

      dragStart.current = { x: event.screenX, y: event.screenY };
      window.addEventListener("mousemove", handleDragMove);
      window.addEventListener("mouseup", clearDragListeners, { once: true });
    },
    [clearDragListeners, handleDragMove],
  );

  return (
    <header className="relative z-10 flex flex-none items-center justify-between gap-3">
      <div className="drag-handle flex min-w-0 flex-1 items-center" onMouseDown={prepareWindowDrag}>
        <div className="min-w-0">
          <p className="m-0 text-[0.72rem] font-black uppercase leading-tight text-amber-300">
            FoxyTunnel
          </p>
          <h1 className="m-0 truncate text-lg font-extrabold leading-tight text-[#fff2e5]">
            Tor Control
          </h1>
        </div>
      </div>

      <div className="flex flex-none items-center gap-2">
        <Chip
          className={`h-8 w-[88px] justify-center border px-2.5 text-[0.78rem] font-extrabold shadow-lg ${meta.chipClass}`}
          variant="soft"
        >
          <StatusIcon className={status === "Bootstrapping" ? "animate-spin" : ""} size={13} />
          {meta.label}
        </Chip>
        {view === "main" ? (
          <Button
            aria-label="Settings"
            className="icon-button"
            isIconOnly
            size="sm"
            variant="outline"
            onPress={onOpenSettings}
          >
            <Settings size={16} />
          </Button>
        ) : null}
        <Button
          aria-label="Close"
          className="icon-button"
          isIconOnly
          size="sm"
          variant="outline"
          onPress={onClose}
        >
          <X size={16} />
        </Button>
      </div>
    </header>
  );
}

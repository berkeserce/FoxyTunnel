import { Button, Chip } from "@heroui/react";
import { Settings, X } from "lucide-react";
import { statusMeta } from "../statusMeta";
import type { ProxyStatus, ViewMode } from "../types";

type AppHeaderProps = {
  status: ProxyStatus;
  view: ViewMode;
  onOpenSettings: () => void;
  onClose: () => void;
};

export function AppHeader({ status, view, onOpenSettings, onClose }: AppHeaderProps) {
  const meta = statusMeta[status];
  const StatusIcon = meta.icon;

  return (
    <header className="relative z-10 flex flex-none items-start justify-between gap-3">
      <div className="flex min-w-0 items-center">
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
          className={`h-8 min-w-[76px] border px-2.5 text-[0.78rem] font-extrabold shadow-lg ${meta.chipClass}`}
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

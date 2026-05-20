import { Button, Chip } from "@heroui/react";
import { Settings, X } from "lucide-react";
import logoUrl from "../../src-tauri/icons/fav1.png";
import type { ProxyStatus, ViewMode } from "../types";

const STATUS_LABELS: Record<ProxyStatus, string> = {
  Bootstrapping: "Starting",
  Error: "Error",
  Running: "Running",
  Stopped: "Idle",
};

const STATUS_CLASSES: Record<ProxyStatus, string> = {
  Bootstrapping: "border-amber-600/70 bg-amber-950/70 text-amber-200",
  Error: "border-red-500/80 bg-red-950/70 text-red-200",
  Running: "border-emerald-600/70 bg-emerald-950/70 text-emerald-200",
  Stopped: "border-zinc-700 bg-[#1d1510] text-[#fff2e5]",
};

type AppHeaderProps = {
  status: ProxyStatus;
  view: ViewMode;
  onOpenSettings: () => void;
  onClose: () => void;
};

export function AppHeader({ status, view, onOpenSettings, onClose }: AppHeaderProps) {
  return (
    <header className="flex flex-none items-start justify-between gap-3">
      <div className="flex min-w-0 items-center gap-3">
        <span className="grid size-10 flex-none place-items-center overflow-hidden rounded-lg border border-orange-800/80 bg-white">
          <img className="size-full object-cover scale-150" src={logoUrl} alt="" />
        </span>
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
          className={`h-8 border px-2.5 text-[0.78rem] font-extrabold ${STATUS_CLASSES[status]}`}
          variant="soft"
        >
          <span className="mr-1.5 inline-block size-1.5 rounded-full bg-current align-[1px]" />
          {STATUS_LABELS[status]}
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

import { AlertTriangle, CheckCircle2, CircleDashed, Power } from "lucide-react";
import type { ProxyStatus } from "./types";

export const statusMeta = {
  Bootstrapping: {
    accent: "amber",
    chipClass: "border-amber-600/70 bg-amber-950/80 text-amber-200",
    glowClass: "bg-amber-500/30",
    health: "Bootstrapping Tor",
    icon: CircleDashed,
    label: "Starting",
    primaryClass: "border-amber-500 bg-amber-500 text-[#130a04] hover:bg-amber-400",
  },
  Error: {
    accent: "red",
    chipClass: "border-red-500/80 bg-red-950/80 text-red-200",
    glowClass: "bg-red-500/30",
    health: "Needs attention",
    icon: AlertTriangle,
    label: "Error",
    primaryClass: "border-orange-500 bg-orange-500 text-[#130a04] hover:bg-orange-400",
  },
  Running: {
    accent: "emerald",
    chipClass: "border-emerald-600/70 bg-emerald-950/80 text-emerald-200",
    glowClass: "bg-emerald-500/25",
    health: "SOCKS proxy active",
    icon: CheckCircle2,
    label: "Running",
    primaryClass:
      "border-red-700/70 bg-[#261b14] text-[#fff2e5] hover:border-red-500 hover:bg-red-950/70",
  },
  Stopped: {
    accent: "zinc",
    chipClass: "border-zinc-700 bg-[#1d1510] text-[#fff2e5]",
    glowClass: "bg-orange-500/15",
    health: "Ready to start",
    icon: Power,
    label: "Idle",
    primaryClass: "border-orange-500 bg-orange-500 text-[#130a04] hover:bg-orange-400",
  },
} satisfies Record<
  ProxyStatus,
  {
    accent: string;
    chipClass: string;
    glowClass: string;
    health: string;
    icon: typeof Power;
    label: string;
    primaryClass: string;
  }
>;

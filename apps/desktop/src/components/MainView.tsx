import { Button } from "@heroui/react";
import { AnimatePresence, motion } from "framer-motion";
import {
  CircleHelp,
  Copy,
  Globe2,
  Loader2,
  Network,
  Play,
  ShieldCheck,
  ShieldX,
  Square,
  WifiOff,
} from "lucide-react";
import { LiveLog } from "./LiveLog";
import { cardVariants, quickTransition, springTransition, viewVariants } from "../motionPresets";
import { statusMeta } from "../statusMeta";
import type { LogLine, ProxyStatus, RoutingMode, SystemProxyStatus, TorRouteStatus } from "../types";

type MainViewProps = {
  endpoint: string;
  exitCountry: string | null;
  routingMode: RoutingMode;
  status: ProxyStatus;
  systemProxy: SystemProxyStatus;
  actionInFlight: boolean;
  torTestInFlight: boolean;
  routeStatus: TorRouteStatus;
  routeIpText: string;
  routeDetailText: string;
  logs: LogLine[];
  isVisible: boolean;
  onPrimaryAction: () => void;
  onRoutingModeChange: (mode: RoutingMode) => void;
  onCopyEndpoint: () => void;
  onTestTor: () => void;
  onClearLogs: () => void;
};

function actionLabel(status: ProxyStatus, actionInFlight: boolean, routingMode: RoutingMode) {
  if (actionInFlight && status !== "Running") {
    return routingMode === "system_proxy" ? "Starting System Proxy..." : "Starting SOCKS...";
  }

  if (status === "Bootstrapping") {
    return "Bootstrapping...";
  }

  if (status === "Running") {
    return "Stop";
  }

  return routingMode === "system_proxy" ? "Start System Proxy" : "Start SOCKS";
}

function statusDetail(status: ProxyStatus, routingMode: RoutingMode) {
  if (status === "Running") {
    return routingMode === "system_proxy"
      ? "Proxy-aware apps use FoxyTunnel through system proxy"
      : "Local SOCKS endpoint is accepting traffic";
  }

  if (status === "Bootstrapping") {
    return "Connecting to the Tor network";
  }

  if (status === "Error") {
    return "Check logs, then try starting again";
  }

  return "No traffic is routed yet";
}

function modeLabel(routingMode: RoutingMode) {
  return routingMode === "system_proxy" ? "System Proxy" : "SOCKS Only";
}

function torIcon(status: TorRouteStatus, inFlight: boolean) {
  if (inFlight) {
    return <Loader2 className="animate-spin" size={15} />;
  }

  if (status === "tor") {
    return <ShieldCheck size={15} />;
  }

  if (status === "not_tor") {
    return <ShieldX size={15} />;
  }

  if (status === "unavailable") {
    return <WifiOff size={15} />;
  }

  return <CircleHelp size={15} />;
}

function torResultMeta(status: TorRouteStatus, text: string, detail: string, inFlight: boolean, isRunning: boolean) {
  if (inFlight) {
    return {
      className: "text-amber-200",
      label: "Checking",
      detail: "Contacting Tor Check",
      text: "Checking active route",
    };
  }

  if (status === "tor") {
    return {
      className: "text-emerald-300",
      label: "Verified",
      detail,
      text,
    };
  }

  if (status === "not_tor") {
    return {
      className: "text-red-300",
      label: "Not Tor",
      detail,
      text,
    };
  }

  if (status === "unavailable") {
    return {
      className: "text-orange-300",
      label: "Unavailable",
      detail,
      text,
    };
  }

  return {
    className: "text-[#b8a494]",
    label: "Not tested",
    detail: isRunning ? "Use Test Tor to verify the active route" : "Start proxy to enable verification",
    text: "Tor route not checked",
  };
}

export function MainView({
  endpoint,
  exitCountry,
  routingMode,
  status,
  systemProxy,
  actionInFlight,
  torTestInFlight,
  routeStatus,
  routeIpText,
  routeDetailText,
  logs,
  isVisible,
  onPrimaryAction,
  onRoutingModeChange,
  onCopyEndpoint,
  onTestTor,
  onClearLogs,
}: MainViewProps) {
  const meta = statusMeta[status];
  const StatusIcon = meta.icon;
  const isRunning = status === "Running";
  const isBootstrapping = status === "Bootstrapping";
  const canStart = status === "Stopped" || status === "Error";
  const systemProxyUnsupported = routingMode === "system_proxy" && !systemProxy.supported;
  const canUsePrimary = !actionInFlight && !isBootstrapping && !systemProxyUnsupported && (isRunning || canStart);
  const modeLocked = actionInFlight || isBootstrapping || isRunning;
  const torResult = torResultMeta(routeStatus, routeIpText, routeDetailText, torTestInFlight, isRunning);
  const proxyAwareText = systemProxy.supported
    ? `${systemProxy.backend} proxy-aware apps only`
    : (systemProxy.message ?? "System Proxy is not supported on this desktop");

  return (
    <AnimatePresence mode="wait">
      {isVisible ? (
        <motion.section
          key="main-view"
          animate="center"
          className="absolute inset-0 flex min-h-0 flex-col gap-3"
          exit="exit"
          initial="enter"
          transition={springTransition}
          variants={viewVariants}
        >
          <motion.section
            animate="visible"
            className="relative overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]/95 p-3 shadow-[0_18px_45px_rgba(0,0,0,0.28)]"
            initial="hidden"
            transition={quickTransition}
            variants={cardVariants}
          >
            <motion.div
              animate={isRunning ? { opacity: [0.35, 0.9, 0.35], scale: [1, 1.16, 1] } : {}}
              className={`pointer-events-none absolute -right-8 -top-8 size-24 rounded-full blur-2xl ${meta.glowClass}`}
              transition={{ duration: 2.2, repeat: isRunning ? Infinity : 0 }}
            />
            <div className="relative flex items-start gap-3">
              <div className="grid size-9 flex-none place-items-center rounded-lg border border-orange-900/70 bg-[#100c09] text-amber-300">
                <StatusIcon className={isBootstrapping ? "animate-spin" : ""} size={18} />
              </div>
              <div className="min-w-0 flex-1">
                <span className="text-[0.68rem] font-black uppercase tracking-wide text-[#b8a494]">
                  Proxy status
                </span>
                <div className="mt-0.5 flex items-center gap-2">
                  <strong className="text-base font-black text-[#fff2e5]">{meta.health}</strong>
                  {isRunning ? <span className="size-2 rounded-full bg-emerald-400 shadow-[0_0_14px_rgba(52,211,153,0.9)]" /> : null}
                </div>
                <p className="mt-1 text-[0.72rem] font-bold text-[#b8a494]">{statusDetail(status, routingMode)}</p>
                <p className="mt-0.5 break-all text-[0.72rem] font-bold text-[#8f7d70]">
                  SOCKS {endpoint}
                  {exitCountry ? ` · Exit: ${exitCountry}` : ""}
                </p>
                <p className="mt-0.5 text-[0.7rem] font-bold text-[#8f7d70]">
                  Mode: {modeLabel(routingMode)}
                  {routingMode === "system_proxy" ? ` · ${proxyAwareText}` : ""}
                </p>
              </div>
              <Button className="ghost-button" size="sm" variant="outline" onPress={onCopyEndpoint}>
                <Copy size={14} />
                Copy
              </Button>
            </div>

            <div className="relative mt-2 grid gap-1.5 border-t border-[#3b2819]/70 pt-2">
              <div className="mode-segment compact" role="radiogroup" aria-label="Routing mode">
                <button
                  aria-checked={routingMode === "socks_only"}
                  className="mode-option"
                  disabled={modeLocked}
                  role="radio"
                  type="button"
                  onClick={() => onRoutingModeChange("socks_only")}
                >
                  <Network size={14} />
                  SOCKS Only
                </button>
                <button
                  aria-checked={routingMode === "system_proxy"}
                  className="mode-option"
                  disabled={modeLocked || !systemProxy.supported}
                  role="radio"
                  type="button"
                  onClick={() => onRoutingModeChange("system_proxy")}
                >
                  <Globe2 size={14} />
                  System Proxy
                </button>
              </div>
              {routingMode === "system_proxy" ? (
                <p
                  className={`m-0 text-[0.66rem] font-bold leading-tight ${
                    systemProxy.supported ? "text-[#b8a494]" : "text-red-200"
                  }`}
                >
                  {systemProxy.supported ? "Routes proxy-aware desktop apps through Tor." : proxyAwareText}
                </p>
              ) : null}
            </div>

            <div className="relative mt-2 flex min-w-0 items-center justify-between gap-2 border-t border-[#3b2819]/70 pt-2">
              <motion.span
                animate={{ opacity: 1, y: 0 }}
                className={`grid min-w-0 gap-0.5 text-[0.72rem] font-extrabold leading-tight ${torResult.className}`}
                initial={{ opacity: 0, y: 3 }}
                key={`${routeStatus}-${routeIpText}-${routeDetailText}-${torTestInFlight}`}
                title={torResult.text}
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  <span className="flex-none opacity-90">{torIcon(routeStatus, torTestInFlight)}</span>
                  <span className="flex-none uppercase opacity-70">{torResult.label}</span>
                  <span className="truncate">{torResult.text}</span>
                </span>
                <span className="ml-[22px] flex min-w-0 items-center gap-2 truncate text-[0.66rem] font-bold opacity-70">
                  <span className="truncate">{torResult.detail}</span>
                </span>
              </motion.span>
              <Button
                className="ghost-button h-8 flex-none px-3"
                isDisabled={torTestInFlight || !isRunning}
                size="sm"
                variant="outline"
                onPress={onTestTor}
              >
                {torTestInFlight ? <Loader2 className="animate-spin" size={14} /> : <ShieldCheck size={14} />}
                Test Tor
              </Button>
            </div>
          </motion.section>

          <motion.div whileTap={{ scale: 0.985 }}>
            <Button
              className={`relative h-12 w-full flex-none overflow-hidden text-base font-black shadow-[0_16px_34px_rgba(249,115,22,0.18)] ${meta.primaryClass}`}
              isDisabled={!canUsePrimary}
              size="lg"
              variant="outline"
              onPress={onPrimaryAction}
            >
              {(actionInFlight || isBootstrapping) && !isRunning ? (
                <Loader2 className="animate-spin" size={16} />
              ) : isRunning ? (
                <Square className="fill-red-500 text-red-500" size={16} />
              ) : (
                <Play size={16} />
              )}
              <span className="relative z-10">{actionLabel(status, actionInFlight, routingMode)}</span>
              {(actionInFlight || isBootstrapping) && !isRunning ? (
                <motion.span
                  animate={{ x: ["-120%", "120%"] }}
                  className="absolute inset-y-0 w-24 rotate-12 bg-white/25 blur-md"
                  transition={{ duration: 1.1, repeat: Infinity, ease: "linear" }}
                />
              ) : null}
            </Button>
          </motion.div>

          <LiveLog lines={logs} onClear={onClearLogs} />
        </motion.section>
      ) : null}
    </AnimatePresence>
  );
}

import { Button } from "@heroui/react";
import { AnimatePresence, motion } from "framer-motion";
import { CheckCircle2, Copy, Loader2, Play, ShieldCheck, ShieldX, Square, WifiOff } from "lucide-react";
import { LiveLog } from "./LiveLog";
import { cardVariants, quickTransition, springTransition, viewVariants } from "../motionPresets";
import { statusMeta } from "../statusMeta";
import type { LogLine, ProxyStatus } from "../types";

type MainViewProps = {
  endpoint: string;
  status: ProxyStatus;
  actionInFlight: boolean;
  torTestInFlight: boolean;
  torTestStatus: string;
  torTestText: string;
  logs: LogLine[];
  isVisible: boolean;
  onPrimaryAction: () => void;
  onCopyEndpoint: () => void;
  onTestTor: () => void;
  onRefresh: () => void;
  onClearLogs: () => void;
};

function actionLabel(status: ProxyStatus, actionInFlight: boolean) {
  if (actionInFlight && status !== "Running") {
    return "Starting...";
  }

  if (status === "Bootstrapping") {
    return "Bootstrapping...";
  }

  return status === "Running" ? "Stop" : "Start";
}

function torIcon(status: string, inFlight: boolean) {
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

  return <CheckCircle2 size={15} />;
}

export function MainView({
  endpoint,
  status,
  actionInFlight,
  torTestInFlight,
  torTestStatus,
  torTestText,
  logs,
  isVisible,
  onPrimaryAction,
  onCopyEndpoint,
  onTestTor,
  onRefresh,
  onClearLogs,
}: MainViewProps) {
  const meta = statusMeta[status];
  const StatusIcon = meta.icon;
  const isRunning = status === "Running";
  const isBootstrapping = status === "Bootstrapping";
  const canStart = status === "Stopped" || status === "Error";
  const canUsePrimary = !actionInFlight && !isBootstrapping && (isRunning || canStart);
  const torResultClass =
    torTestStatus === "tor"
      ? "border-emerald-700/70 bg-emerald-950/35 text-emerald-200"
      : torTestStatus === "not_tor" || torTestStatus === "unavailable"
        ? "border-red-700/70 bg-red-950/35 text-red-200"
        : "border-[#3b2819] bg-[#100c09] text-[#b8a494]";

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
              <div className="grid size-11 flex-none place-items-center rounded-lg border border-orange-900/70 bg-[#100c09] text-amber-300">
                <StatusIcon className={isBootstrapping ? "animate-spin" : ""} size={21} />
              </div>
              <div className="min-w-0 flex-1">
                <span className="text-[0.72rem] font-black uppercase tracking-wide text-[#b8a494]">
                  Connection health
                </span>
                <div className="mt-1 flex items-center gap-2">
                  <strong className="text-lg font-black text-[#fff2e5]">{meta.health}</strong>
                  {isRunning ? <span className="size-2 rounded-full bg-emerald-400 shadow-[0_0_14px_rgba(52,211,153,0.9)]" /> : null}
                </div>
                <p className="mt-1 truncate text-[0.78rem] font-bold text-[#b8a494]">{endpoint}</p>
              </div>
              <Button className="ghost-button" size="sm" variant="outline" onPress={onCopyEndpoint}>
                <Copy size={14} />
                Copy
              </Button>
            </div>

            <div className="relative mt-3 flex min-w-0 items-center gap-2">
              <Button
                className="ghost-button"
                isDisabled={torTestInFlight || !isRunning}
                size="sm"
                variant="outline"
                onPress={onTestTor}
              >
                {torTestInFlight ? <Loader2 className="animate-spin" size={14} /> : <ShieldCheck size={14} />}
                {torTestInFlight ? "Testing..." : "Test Tor"}
              </Button>
              <motion.span
                animate={{ opacity: 1, y: 0 }}
                className={`flex min-w-0 items-center gap-1.5 truncate rounded-md border px-2 py-1 text-[0.76rem] font-extrabold ${torResultClass}`}
                initial={{ opacity: 0, y: 4 }}
                key={`${torTestStatus}-${torTestText}`}
                title={torTestText}
              >
                <span className="flex-none">{torIcon(torTestStatus, torTestInFlight)}</span>
                <span className="truncate">{torTestText}</span>
              </motion.span>
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
                <Square size={16} />
              ) : (
                <Play size={16} />
              )}
              <span className="relative z-10">{actionLabel(status, actionInFlight)}</span>
              {(actionInFlight || isBootstrapping) && !isRunning ? (
                <motion.span
                  animate={{ x: ["-120%", "120%"] }}
                  className="absolute inset-y-0 w-24 rotate-12 bg-white/25 blur-md"
                  transition={{ duration: 1.1, repeat: Infinity, ease: "linear" }}
                />
              ) : null}
            </Button>
          </motion.div>

          <LiveLog lines={logs} onClear={onClearLogs} onRefresh={onRefresh} />
        </motion.section>
      ) : null}
    </AnimatePresence>
  );
}

import { Button } from "@heroui/react";
import { Copy, Play, Square } from "lucide-react";
import { LiveLog } from "./LiveLog";
import type { ProxyStatus } from "../types";

type MainViewProps = {
  endpoint: string;
  status: ProxyStatus;
  actionInFlight: boolean;
  torTestInFlight: boolean;
  torTestStatus: string;
  torTestText: string;
  logs: string[];
  onPrimaryAction: () => void;
  onCopyEndpoint: () => void;
  onTestTor: () => void;
  onRefresh: () => void;
  onClearLogs: () => void;
};

function actionLabel(status: ProxyStatus) {
  if (status === "Bootstrapping") {
    return "Bootstrapping...";
  }

  return status === "Running" ? "Stop" : "Start";
}

export function MainView({
  endpoint,
  status,
  actionInFlight,
  torTestInFlight,
  torTestStatus,
  torTestText,
  logs,
  onPrimaryAction,
  onCopyEndpoint,
  onTestTor,
  onRefresh,
  onClearLogs,
}: MainViewProps) {
  const isRunning = status === "Running";
  const isBootstrapping = status === "Bootstrapping";
  const canStart = status === "Stopped" || status === "Error";
  const canUsePrimary = !actionInFlight && !isBootstrapping && (isRunning || canStart);

  return (
    <section className="flex min-h-0 flex-1 flex-col gap-3">
      <section className="rounded-lg border border-[#3b2819] bg-[#15100c] p-3">
        <div className="flex min-w-0 items-start justify-between gap-3">
          <div className="min-w-0">
            <span className="text-[0.76rem] font-black text-[#b8a494]">Endpoint</span>
            <strong className="mt-1 block break-words text-base font-extrabold text-[#fff2e5]">
              {endpoint}
            </strong>
          </div>
          <Button className="ghost-button" size="sm" variant="outline" onPress={onCopyEndpoint}>
            <Copy size={14} />
            Copy
          </Button>
        </div>
        <div className="mt-3 flex min-w-0 items-center gap-2">
          <Button
            className="ghost-button"
            isDisabled={torTestInFlight || !isRunning}
            size="sm"
            variant="outline"
            onPress={onTestTor}
          >
            {torTestInFlight ? "Testing..." : "Test Tor"}
          </Button>
          <span
            className={`min-w-0 truncate text-[0.78rem] font-extrabold ${torTestStatus === "tor" ? "text-emerald-300" : torTestStatus === "not_tor" || torTestStatus === "unavailable" ? "text-red-200" : "text-[#b8a494]"}`}
            title={torTestText}
          >
            {torTestText}
          </span>
        </div>
      </section>

      <Button
        className={`h-12 flex-none text-base font-black ${isRunning ? "border-red-700/70 bg-[#261b14] text-[#fff2e5] hover:border-red-500 hover:bg-red-950/70" : "border-orange-500 bg-orange-500 text-[#130a04] hover:bg-orange-400"}`}
        isDisabled={!canUsePrimary}
        size="lg"
        variant="outline"
        onPress={onPrimaryAction}
      >
        {isRunning ? <Square size={16} /> : <Play size={16} />}
        {actionLabel(status)}
      </Button>

      <LiveLog lines={logs} onClear={onClearLogs} onRefresh={onRefresh} />
    </section>
  );
}

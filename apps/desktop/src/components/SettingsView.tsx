import { Button, Tooltip } from "@heroui/react";
import { AnimatePresence, motion } from "framer-motion";
import {
  ChevronLeft,
  Clock3,
  DatabaseZap,
  FolderOpen,
  Globe2,
  Info,
  LockKeyhole,
  SlidersHorizontal,
} from "lucide-react";
import { useEffect, useState } from "react";
import { cardVariants, quickTransition, springTransition, viewVariants } from "../motionPresets";
import { statusMeta } from "../statusMeta";
import type { ProxyStatus, StartOptions } from "../types";

const EXIT_COUNTRY_OPTIONS = [
  { label: "Auto", value: "" },
  { label: "Turkey", value: "TR" },
  { label: "Germany", value: "DE" },
  { label: "Netherlands", value: "NL" },
  { label: "France", value: "FR" },
  { label: "United Kingdom", value: "GB" },
  { label: "United States", value: "US" },
  { label: "Canada", value: "CA" },
  { label: "Sweden", value: "SE" },
];

const TIMEOUT_PRESETS = [60, 120, 300];

type SettingsViewProps = {
  settings: StartOptions;
  status: ProxyStatus;
  isDisabled: boolean;
  isVisible: boolean;
  resetInFlight: boolean;
  onBack: () => void;
  onOpenLogFolder: () => void;
  onResetTorData: () => void;
  onSettingsChange: (settings: StartOptions) => void;
};

export function SettingsView({
  settings,
  status,
  isDisabled,
  isVisible,
  resetInFlight,
  onBack,
  onOpenLogFolder,
  onResetTorData,
  onSettingsChange,
}: SettingsViewProps) {
  const meta = statusMeta[status];
  const [resetArmed, setResetArmed] = useState(false);

  useEffect(() => {
    if (!isVisible || isDisabled || resetInFlight) {
      setResetArmed(false);
    }
  }, [isDisabled, isVisible, resetInFlight]);

  return (
    <AnimatePresence mode="wait">
      {isVisible ? (
        <motion.section
          key="settings-view"
          animate="center"
          className="absolute inset-0 flex min-h-0 flex-col gap-3"
          exit="exit"
          initial="enter"
          transition={springTransition}
          variants={viewVariants}
        >
          <div className="flex flex-none items-center gap-3">
            <Button
              aria-label="Back"
              className="ghost-button"
              isIconOnly
              size="sm"
              variant="outline"
              onPress={onBack}
            >
              <ChevronLeft size={16} />
            </Button>
            <div>
              <h2 className="m-0 text-base font-extrabold text-[#fff2e5]">Settings</h2>
              <p className="m-0 text-[0.72rem] font-bold text-[#b8a494]">
                {isDisabled ? "Locked while proxy is active" : "Changes are saved automatically"}
              </p>
            </div>
          </div>

          <div className="min-h-0 overflow-y-auto pr-1">
            <div className="grid gap-3 pb-1">
              <motion.section
                animate="visible"
                className="relative grid gap-3 overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]/95 p-3 shadow-[0_18px_45px_rgba(0,0,0,0.24)]"
                initial="hidden"
                transition={quickTransition}
                variants={cardVariants}
              >
                <div className={`pointer-events-none absolute -right-10 -top-10 size-28 rounded-full blur-2xl ${meta.glowClass}`} />
                <SectionHeader
                  detail={isDisabled ? `${meta.label} mode is using the current values` : "Port and local SOCKS logging"}
                  icon={isDisabled ? LockKeyhole : SlidersHorizontal}
                  title={isDisabled ? "Settings locked" : "Proxy settings"}
                />

                <div className="relative grid gap-1.5">
                  <LabelWithInfo text="Port" tooltip="Local SOCKS port. Configure your browser to use this port." />
                  <input
                    className="fox-input"
                    disabled={isDisabled}
                    max={65535}
                    min={1}
                    type="number"
                    value={String(settings.socks_port)}
                    onChange={(event) =>
                      onSettingsChange({
                        ...settings,
                        socks_port: Number(event.currentTarget.value),
                      })
                    }
                  />
                </div>

                <button
                  aria-checked={settings.log_connections}
                  className="foxy-switch"
                  disabled={isDisabled}
                  role="switch"
                  type="button"
                  onClick={() =>
                    onSettingsChange({
                      ...settings,
                      log_connections: !settings.log_connections,
                    })
                  }
                >
                  <span className="min-w-0">
                    <span className="block text-sm font-extrabold text-[#fff2e5]">Log connections</span>
                    <span className="block text-[0.72rem] font-bold text-[#b8a494]">
                      Show SOCKS CONNECT targets in the live log
                    </span>
                  </span>
                  <span className="foxy-switch-track" aria-hidden="true">
                    <span className="foxy-switch-thumb" />
                  </span>
                </button>

                <Button
                  className="ghost-button h-9 w-full"
                  size="sm"
                  variant="outline"
                  onPress={onOpenLogFolder}
                >
                  <FolderOpen size={14} />
                  Open Logs
                </Button>
              </motion.section>

              <motion.section
                animate="visible"
                className="relative grid gap-3 overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]/95 p-3 shadow-[0_18px_45px_rgba(0,0,0,0.24)]"
                initial="hidden"
                transition={quickTransition}
                variants={cardVariants}
              >
                <SectionHeader
                  detail={settings.exit_country ? `Strict exit: ${settings.exit_country}` : "Automatic exit selection"}
                  icon={Globe2}
                  title="Tor Control"
                />

                <div className="relative grid gap-1.5">
                  <LabelWithInfo
                    text="Exit country"
                    tooltip="Strict country selection for Tor exit relays. If no matching exit is available, the connection fails."
                  />
                  <select
                    className="fox-input"
                    disabled={isDisabled}
                    value={settings.exit_country ?? ""}
                    onChange={(event) =>
                      onSettingsChange({
                        ...settings,
                        exit_country: event.currentTarget.value || null,
                      })
                    }
                  >
                    {EXIT_COUNTRY_OPTIONS.map((country) => (
                      <option key={country.value || "auto"} value={country.value}>
                        {country.label}
                        {country.value ? ` (${country.value})` : ""}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="relative grid gap-1.5">
                  <LabelWithInfo
                    text="Bootstrap"
                    tooltip="Maximum time FoxyTunnel waits while connecting to the Tor network."
                  />
                  <div className="grid grid-cols-3 gap-1.5">
                    {TIMEOUT_PRESETS.map((seconds) => (
                      <Button
                        key={seconds}
                        className={`ghost-button h-8 px-2 text-xs ${settings.bootstrap_timeout_seconds === seconds ? "border-orange-500 text-amber-200" : ""}`}
                        isDisabled={isDisabled}
                        size="sm"
                        variant="outline"
                        onPress={() =>
                          onSettingsChange({
                            ...settings,
                            bootstrap_timeout_seconds: seconds,
                          })
                        }
                      >
                        <Clock3 size={13} />
                        {seconds}s
                      </Button>
                    ))}
                  </div>
                  <input
                    className="fox-input"
                    disabled={isDisabled}
                    max={600}
                    min={10}
                    type="number"
                    value={String(settings.bootstrap_timeout_seconds)}
                    onChange={(event) =>
                      onSettingsChange({
                        ...settings,
                        bootstrap_timeout_seconds: Number(event.currentTarget.value),
                      })
                    }
                  />
                </div>

                <Button
                  className={`danger-button h-10 w-full ${resetArmed ? "danger-button-armed" : ""} ${resetInFlight ? "danger-button-busy" : ""}`}
                  isDisabled={isDisabled || resetInFlight}
                  size="sm"
                  variant="outline"
                  onPress={() => {
                    if (!resetArmed) {
                      setResetArmed(true);
                      return;
                    }

                    setResetArmed(false);
                    onResetTorData();
                  }}
                >
                  <DatabaseZap size={14} />
                  {resetInFlight ? "Resetting..." : resetArmed ? "Confirm Reset" : "Reset Tor Data"}
                </Button>
              </motion.section>
            </div>
          </div>
        </motion.section>
      ) : null}
    </AnimatePresence>
  );
}

type SectionHeaderProps = {
  title: string;
  detail: string;
  icon: typeof SlidersHorizontal;
};

function SectionHeader({ title, detail, icon: Icon }: SectionHeaderProps) {
  return (
    <div className="relative flex items-center gap-2">
      <div className="grid size-10 flex-none place-items-center rounded-lg border border-orange-900/70 bg-[#1d1510] text-amber-300">
        <Icon size={19} />
      </div>
      <div className="min-w-0">
        <strong className="block text-sm font-black text-[#fff2e5]">{title}</strong>
        <span className="block truncate text-[0.76rem] font-bold text-[#b8a494]">{detail}</span>
      </div>
    </div>
  );
}

type LabelWithInfoProps = {
  text: string;
  tooltip: string;
};

function LabelWithInfo({ text, tooltip }: LabelWithInfoProps) {
  return (
    <div className="flex items-center gap-1.5 text-[0.76rem] font-black text-[#b8a494]">
      <span>{text}</span>
      <Tooltip delay={150}>
        <Tooltip.Trigger>
          <span className="grid size-4 place-items-center rounded-full border border-orange-800/80 bg-[#1d1510] text-amber-300 transition hover:border-orange-500 hover:text-[#fff2e5]">
            <Info size={11} />
          </span>
        </Tooltip.Trigger>
        <Tooltip.Content className="max-w-[245px] border border-orange-900/80 bg-[#100b08] text-xs font-bold text-[#fff2e5]" placement="top">
          {tooltip}
        </Tooltip.Content>
      </Tooltip>
    </div>
  );
}

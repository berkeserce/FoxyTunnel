import { Button, Input, Switch, Tooltip } from "@heroui/react";
import { AnimatePresence, motion } from "framer-motion";
import { ChevronLeft, Info, LockKeyhole, SlidersHorizontal } from "lucide-react";
import { cardVariants, quickTransition, springTransition, viewVariants } from "../motionPresets";
import { statusMeta } from "../statusMeta";
import type { ProxyStatus, StartOptions } from "../types";

type SettingsViewProps = {
  settings: StartOptions;
  status: ProxyStatus;
  isDisabled: boolean;
  isVisible: boolean;
  onBack: () => void;
  onSettingsChange: (settings: StartOptions) => void;
};

export function SettingsView({
  settings,
  status,
  isDisabled,
  isVisible,
  onBack,
  onSettingsChange,
}: SettingsViewProps) {
  const meta = statusMeta[status];

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

          <motion.section
            animate="visible"
            className="relative grid gap-3 overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]/95 p-3 shadow-[0_18px_45px_rgba(0,0,0,0.24)]"
            initial="hidden"
            transition={quickTransition}
            variants={cardVariants}
          >
            <div className={`pointer-events-none absolute -right-10 -top-10 size-28 rounded-full blur-2xl ${meta.glowClass}`} />
            <div className="relative flex items-center gap-2 rounded-lg border border-[#3b2819] bg-[#100c09] p-3">
              <div className="grid size-10 flex-none place-items-center rounded-lg border border-orange-900/70 bg-[#1d1510] text-amber-300">
                {isDisabled ? <LockKeyhole size={19} /> : <SlidersHorizontal size={19} />}
              </div>
              <div className="min-w-0">
                <strong className="block text-sm font-black text-[#fff2e5]">
                  {isDisabled ? "Settings locked" : "Proxy settings"}
                </strong>
                <span className="block truncate text-[0.76rem] font-bold text-[#b8a494]">
                  {isDisabled ? `${meta.label} mode is using the current values` : "Port, timeout and connection logging"}
                </span>
              </div>
            </div>

            <div className="relative grid gap-1.5">
              <LabelWithInfo text="Port" tooltip="Local SOCKS port. Configure your browser to use this port." />
              <Input
                className="fox-input"
                disabled={isDisabled}
                fullWidth
                max={65535}
                min={1}
                type="number"
                value={String(settings.socks_port)}
                variant="secondary"
                onChange={(event) =>
                  onSettingsChange({
                    ...settings,
                    socks_port: Number(event.currentTarget.value),
                  })
                }
              />
            </div>

            <div className="relative grid gap-1.5">
              <LabelWithInfo
                text="Bootstrap"
                tooltip="Maximum time FoxyTunnel waits while connecting to the Tor network."
              />
              <Input
                className="fox-input"
                disabled={isDisabled}
                fullWidth
                max={600}
                min={10}
                type="number"
                value={String(settings.bootstrap_timeout_seconds)}
                variant="secondary"
                onChange={(event) =>
                  onSettingsChange({
                    ...settings,
                    bootstrap_timeout_seconds: Number(event.currentTarget.value),
                  })
                }
              />
            </div>

            <Switch
              className="relative text-sm font-extrabold text-[#fff2e5]"
              isDisabled={isDisabled}
              isSelected={settings.log_connections}
              onChange={(isSelected) =>
                onSettingsChange({
                  ...settings,
                  log_connections: isSelected,
                })
              }
            >
              Log connections
            </Switch>
          </motion.section>
        </motion.section>
      ) : null}
    </AnimatePresence>
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

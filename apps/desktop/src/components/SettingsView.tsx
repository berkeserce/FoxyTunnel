import { Button, Input, Switch, Tooltip } from "@heroui/react";
import { ChevronLeft, Info } from "lucide-react";
import type { StartOptions } from "../types";

type SettingsViewProps = {
  settings: StartOptions;
  isDisabled: boolean;
  onBack: () => void;
  onSettingsChange: (settings: StartOptions) => void;
};

export function SettingsView({ settings, isDisabled, onBack, onSettingsChange }: SettingsViewProps) {
  return (
    <section className="flex min-h-0 flex-1 flex-col gap-3">
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
        <h2 className="m-0 text-base font-extrabold text-[#fff2e5]">Settings</h2>
      </div>

      <section className="grid gap-3 rounded-lg border border-[#3b2819] bg-[#15100c] p-3">
        <div className="grid gap-1.5">
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

        <div className="grid gap-1.5">
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
          className="text-sm font-extrabold text-[#fff2e5]"
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
      </section>
    </section>
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
          <span className="grid size-4 place-items-center rounded-full border border-orange-800/80 bg-[#1d1510] text-[0.65rem] text-amber-300">
            <Info size={11} />
          </span>
        </Tooltip.Trigger>
        <Tooltip.Content className="max-w-[250px] border border-orange-900/80 bg-[#100b08] text-xs font-bold text-[#fff2e5]">
          {tooltip}
        </Tooltip.Content>
      </Tooltip>
    </div>
  );
}

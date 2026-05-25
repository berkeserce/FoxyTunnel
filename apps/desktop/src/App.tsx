import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { AppHeader } from "./components/AppHeader";
import { MainView } from "./components/MainView";
import { SettingsView } from "./components/SettingsView";
import type {
  LogDto,
  LogLine,
  LogsDto,
  StartOptions,
  StatusDto,
  TorCheckDto,
  TorRouteStatus,
  ViewMode,
} from "./types";

const MAX_VISIBLE_LOG_LINES = 120;
const LOG_POLL_INTERVAL_MS = 1500;
const SETTINGS_SAVE_DELAY_MS = 450;

const DEFAULT_STATUS: StatusDto = {
  status: "Stopped",
  endpoint: "127.0.0.1:19050",
  socks_port: 19050,
  log_connections: false,
  exit_country: null,
  bootstrap_timeout_seconds: 120,
  last_error: null,
};

function isEditableStatus(status: StatusDto["status"]) {
  return status === "Stopped" || status === "Error";
}

function optionsFromStatus(status: StatusDto): StartOptions {
  return {
    socks_port: status.socks_port,
    log_connections: status.log_connections,
    exit_country: status.exit_country ?? null,
    bootstrap_timeout_seconds: status.bootstrap_timeout_seconds,
  };
}

function formatLogLine(message: string, level = "info") {
  const time = new Date().toLocaleTimeString();
  const prefix = level === "error" ? "ERROR" : level.toUpperCase();

  return `[${time}] ${prefix}: ${message}`;
}

export default function App() {
  const [status, setStatus] = useState<StatusDto>(DEFAULT_STATUS);
  const [settings, setSettings] = useState<StartOptions>(optionsFromStatus(DEFAULT_STATUS));
  const [view, setView] = useState<ViewMode>("main");
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [actionInFlight, setActionInFlight] = useState(false);
  const [torTestInFlight, setTorTestInFlight] = useState(false);
  const [resetInFlight, setResetInFlight] = useState(false);
  const [routeIpText, setRouteIpText] = useState("Tor route not checked");
  const [routeDetailText, setRouteDetailText] = useState("Start proxy to enable verification");
  const [routeStatus, setRouteStatus] = useState<TorRouteStatus>("untested");
  const lastLogSequence = useRef(0);
  const localLogSequence = useRef(0);
  const settingsSaveTimer = useRef<number | undefined>(undefined);
  const statusRef = useRef(status);
  const actionInFlightRef = useRef(actionInFlight);

  useEffect(() => {
    statusRef.current = status;
  }, [status]);

  useEffect(() => {
    actionInFlightRef.current = actionInFlight;
  }, [actionInFlight]);

  const writeLog = useCallback((message: string, level = "info") => {
    localLogSequence.current += 1;
    const id = Number(`${Date.now()}${localLogSequence.current}`.slice(-12));

    setLogs((current) =>
      [
        ...current,
        {
          id,
          level,
          text: formatLogLine(message, level),
        },
      ].slice(-MAX_VISIBLE_LOG_LINES),
    );

    invoke("append_activity_log", { level, message }).catch(() => {});
  }, []);

  const appendBackendLog = useCallback(
    (entry: LogDto) => {
      if (entry.sequence <= lastLogSequence.current) {
        return;
      }

      lastLogSequence.current = entry.sequence;
      setLogs((current) =>
        [
          ...current,
          {
            id: entry.sequence,
            level: entry.level,
            text: formatLogLine(entry.message, entry.level),
          },
        ].slice(-MAX_VISIBLE_LOG_LINES),
      );
    },
    [],
  );

  const renderStatus = useCallback((nextStatus: StatusDto, syncSettings = false) => {
    setStatus(nextStatus);

    if (syncSettings) {
      setSettings(optionsFromStatus(nextStatus));
    }

    if (nextStatus.last_error) {
      writeLog(nextStatus.last_error, "error");
    }
  }, [writeLog]);

  const syncActivityLogs = useCallback(async () => {
    const activity = await invoke<LogsDto>("get_activity_logs");

    for (const entry of activity.entries) {
      appendBackendLog(entry);
    }
  }, [appendBackendLog]);

  const refreshStatus = useCallback(
    async (syncSettings = false) => {
      const nextStatus = await invoke<StatusDto>("get_status");
      renderStatus(nextStatus, syncSettings);
      await syncActivityLogs();
      writeLog(`Status refreshed: ${nextStatus.status} (${nextStatus.endpoint})`);
    },
    [renderStatus, syncActivityLogs, writeLog],
  );

  const saveSettings = useCallback(
    async (nextSettings: StartOptions) => {
      const currentStatus = statusRef.current.status;

      if (!isEditableStatus(currentStatus) || actionInFlightRef.current) {
        return;
      }

      const nextStatus = await invoke<StatusDto>("save_settings", {
        options: nextSettings,
      });
      renderStatus(nextStatus, true);
    },
    [renderStatus],
  );

  const scheduleSettingsSave = useCallback(
    (nextSettings: StartOptions) => {
      const currentStatus = statusRef.current.status;

      if (!isEditableStatus(currentStatus) || actionInFlightRef.current) {
        return;
      }

      window.clearTimeout(settingsSaveTimer.current);
      settingsSaveTimer.current = window.setTimeout(() => {
        saveSettings(nextSettings).catch((error) => writeLog(String(error), "error"));
      }, SETTINGS_SAVE_DELAY_MS);
    },
    [saveSettings, writeLog],
  );

  const handleSettingsChange = useCallback(
    (nextSettings: StartOptions) => {
      setSettings(nextSettings);
      scheduleSettingsSave(nextSettings);
    },
    [scheduleSettingsSave],
  );

  const startSocks = useCallback(async () => {
    setActionInFlight(true);
    writeLog("Starting SOCKS proxy...");

    try {
      const nextStatus = await invoke<StatusDto>("start_socks", {
        options: settings,
      });
      renderStatus(nextStatus, true);
      setRouteStatus("untested");
      setRouteIpText("Tor route not checked");
      setRouteDetailText("Use Test Tor to verify the active route");
      writeLog(`SOCKS proxy running on ${nextStatus.endpoint}`);
    } finally {
      setActionInFlight(false);
    }
  }, [renderStatus, settings, writeLog]);

  const stopSocks = useCallback(async () => {
    setActionInFlight(true);
    writeLog("Stopping SOCKS proxy...");

    try {
      const nextStatus = await invoke<StatusDto>("stop_socks");
      renderStatus(nextStatus);
      setRouteStatus("untested");
      setRouteIpText("Tor route not checked");
      setRouteDetailText("Start proxy to enable verification");
      writeLog("SOCKS proxy stopped.");
    } finally {
      setActionInFlight(false);
    }
  }, [renderStatus, writeLog]);

  const handlePrimaryAction = useCallback(async () => {
    try {
      if (status.status === "Running") {
        await stopSocks();
      } else if (isEditableStatus(status.status)) {
        await startSocks();
      }
    } catch (error) {
      writeLog(String(error), "error");
      refreshStatus().catch(() => {});
    }
  }, [refreshStatus, startSocks, status.status, stopSocks, writeLog]);

  const copyEndpoint = useCallback(async () => {
    await copyText(status.endpoint);
    writeLog("Endpoint copied.");
  }, [status.endpoint, writeLog]);

  const resetTorData = useCallback(async () => {
    setResetInFlight(true);
    writeLog("Resetting Tor data...");

    try {
      await invoke("reset_tor_data");
    } catch (error) {
      writeLog(String(error), "error");
      refreshStatus().catch(() => {});
    } finally {
      setResetInFlight(false);
    }
  }, [refreshStatus, writeLog]);

  const openLogFolder = useCallback(async () => {
    try {
      await invoke<string>("open_log_folder");
      writeLog("Log folder opened.");
    } catch (error) {
      writeLog(String(error), "error");
    }
  }, [writeLog]);

  async function copyText(text: string) {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
    } else {
      const textArea = document.createElement("textarea");
      textArea.value = text;
      textArea.setAttribute("readonly", "");
      textArea.style.position = "fixed";
      textArea.style.opacity = "0";
      document.body.append(textArea);
      textArea.select();

      try {
        document.execCommand("copy");
      } finally {
        textArea.remove();
      }
    }
  }

  const testTorConnection = useCallback(async () => {
    if (status.status !== "Running") {
      setRouteStatus("untested");
      setRouteIpText("Tor route not checked");
      setRouteDetailText("Start proxy to enable verification");
      return;
    }

    setTorTestInFlight(true);
    setRouteIpText("Checking active route");
    setRouteDetailText("Contacting Tor Check");
    setRouteStatus("untested");

    try {
      const result = await invoke<TorCheckDto>("test_tor_connection");
      setRouteIpText(result.ip ?? result.message);
      setRouteDetailText(result.latency_ms ? `${result.latency_ms} ms latency` : "Latency unavailable");
      setRouteStatus(result.status);
    } catch (error) {
      setRouteIpText("Tor check unavailable");
      setRouteDetailText(String(error));
      setRouteStatus("unavailable");
      writeLog(String(error), "error");
    } finally {
      setTorTestInFlight(false);
    }
  }, [status.status, writeLog]);

  const clearActivityLogs = useCallback(async () => {
    await invoke("clear_activity_logs");
    lastLogSequence.current = 0;
    setLogs([]);
  }, []);

  const hidePanel = useCallback(() => {
    invoke("hide_panel_window").catch((error) => writeLog(String(error), "error"));
  }, [writeLog]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<LogDto>("proxy-log", (event) => appendBackendLog(event.payload))
      .then((cleanup) => {
        unlisten = cleanup;
      })
      .catch((error) => writeLog(String(error), "error"));

    return () => {
      unlisten?.();
    };
  }, [appendBackendLog, writeLog]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      syncActivityLogs().catch((error) => writeLog(String(error), "error"));
    }, LOG_POLL_INTERVAL_MS);

    return () => window.clearInterval(timer);
  }, [syncActivityLogs, writeLog]);

  useEffect(() => {
    refreshStatus(true).catch((error) => writeLog(String(error), "error"));
  }, [refreshStatus, writeLog]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }

      if (view === "settings") {
        setView("main");
        return;
      }

      hidePanel();
    };

    window.addEventListener("keydown", onKeyDown);

    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hidePanel, view]);

  const settingsDisabled = !isEditableStatus(status.status) || actionInFlight;

  return (
    <main className="relative flex h-screen w-full flex-col gap-3 overflow-hidden rounded-lg border border-orange-800/80 bg-[#0c0907] p-3.5 text-[#fff2e5]">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_20%_0%,rgba(249,115,22,0.18),transparent_34%),radial-gradient(circle_at_80%_12%,rgba(34,197,94,0.09),transparent_28%)]" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-orange-400/70 to-transparent" />
      <AppHeader
        status={status.status}
        view={view}
        onClose={hidePanel}
        onOpenSettings={() => setView("settings")}
      />

      <div className="relative min-h-0 flex-1">
        <MainView
          actionInFlight={actionInFlight}
          endpoint={status.endpoint}
          isVisible={view === "main"}
          logs={logs}
          exitCountry={status.exit_country ?? null}
          status={status.status}
          torTestInFlight={torTestInFlight}
          routeDetailText={routeDetailText}
          routeIpText={routeIpText}
          routeStatus={routeStatus}
          onClearLogs={() => {
            clearActivityLogs().catch((error) => writeLog(String(error), "error"));
          }}
          onCopyEndpoint={() => {
            copyEndpoint().catch((error) => writeLog(String(error), "error"));
          }}
          onPrimaryAction={handlePrimaryAction}
          onTestTor={testTorConnection}
        />
        <SettingsView
          isDisabled={settingsDisabled}
          isVisible={view === "settings"}
          resetInFlight={resetInFlight}
          settings={settings}
          status={status.status}
          onBack={() => setView("main")}
          onOpenLogFolder={openLogFolder}
          onResetTorData={resetTorData}
          onSettingsChange={handleSettingsChange}
        />
      </div>
    </main>
  );
}

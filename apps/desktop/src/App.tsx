import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { AppHeader } from "./components/AppHeader";
import { MainView } from "./components/MainView";
import { SettingsView } from "./components/SettingsView";
import type { LogDto, LogsDto, StartOptions, StatusDto, TorCheckDto, ViewMode } from "./types";

const MAX_VISIBLE_LOG_LINES = 120;
const LOG_POLL_INTERVAL_MS = 1500;
const SETTINGS_SAVE_DELAY_MS = 450;

const DEFAULT_STATUS: StatusDto = {
  status: "Stopped",
  endpoint: "127.0.0.1:19050",
  socks_port: 19050,
  log_connections: false,
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
  const [logs, setLogs] = useState<string[]>([]);
  const [actionInFlight, setActionInFlight] = useState(false);
  const [torTestInFlight, setTorTestInFlight] = useState(false);
  const [torTestText, setTorTestText] = useState("Not tested");
  const [torTestStatus, setTorTestStatus] = useState("");
  const lastLogSequence = useRef(0);
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
    setLogs((current) => [...current, formatLogLine(message, level)].slice(-MAX_VISIBLE_LOG_LINES));
  }, []);

  const appendBackendLog = useCallback(
    (entry: LogDto) => {
      if (entry.sequence <= lastLogSequence.current) {
        return;
      }

      lastLogSequence.current = entry.sequence;
      writeLog(entry.message, entry.level);
    },
    [writeLog],
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
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(status.endpoint);
    } else {
      const textArea = document.createElement("textarea");
      textArea.value = status.endpoint;
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

    writeLog("Endpoint copied.");
  }, [status.endpoint, writeLog]);

  const testTorConnection = useCallback(async () => {
    if (status.status !== "Running") {
      setTorTestText("Start proxy first");
      setTorTestStatus("unavailable");
      return;
    }

    setTorTestInFlight(true);
    setTorTestText("Testing...");
    setTorTestStatus("");

    try {
      const result = await invoke<TorCheckDto>("test_tor_connection");
      setTorTestText(result.status === "tor" && result.ip ? `IP: ${result.ip}` : result.message);
      setTorTestStatus(result.status);
    } catch (error) {
      setTorTestText(String(error));
      setTorTestStatus("unavailable");
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
    <main className="flex h-screen w-full flex-col gap-3 overflow-y-auto rounded-lg border border-orange-800/80 bg-[#0c0907] p-3.5 text-[#fff2e5]">
      <AppHeader
        status={status.status}
        view={view}
        onClose={hidePanel}
        onOpenSettings={() => setView("settings")}
      />

      {view === "main" ? (
        <MainView
          actionInFlight={actionInFlight}
          endpoint={status.endpoint}
          logs={logs}
          status={status.status}
          torTestInFlight={torTestInFlight}
          torTestStatus={torTestStatus}
          torTestText={torTestText}
          onClearLogs={() => {
            clearActivityLogs().catch((error) => writeLog(String(error), "error"));
          }}
          onCopyEndpoint={() => {
            copyEndpoint().catch((error) => writeLog(String(error), "error"));
          }}
          onPrimaryAction={handlePrimaryAction}
          onRefresh={() => {
            refreshStatus().catch((error) => writeLog(String(error), "error"));
          }}
          onTestTor={testTorConnection}
        />
      ) : (
        <SettingsView
          isDisabled={settingsDisabled}
          settings={settings}
          onBack={() => setView("main")}
          onSettingsChange={handleSettingsChange}
        />
      )}
    </main>
  );
}

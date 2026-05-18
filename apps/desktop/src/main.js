import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

const statusPill = document.querySelector("#status-pill");
const statusText = document.querySelector("#status-text");
const endpointText = document.querySelector("#endpoint-text");
const timeoutText = document.querySelector("#timeout-text");
const messageLog = document.querySelector("#message-log");
const startButton = document.querySelector("#start-button");
const stopButton = document.querySelector("#stop-button");
const refreshButton = document.querySelector("#refresh-button");
const clearLogButton = document.querySelector("#clear-log-button");
const portInput = document.querySelector("#port-input");
const timeoutInput = document.querySelector("#timeout-input");
const logInput = document.querySelector("#log-input");

function writeLog(message, level = "info") {
  const time = new Date().toLocaleTimeString();
  const prefix = level === "error" ? "ERROR" : level.toUpperCase();
  messageLog.textContent = `[${time}] ${prefix}: ${message}\n${messageLog.textContent}`;
}

function renderStatus(status, { syncSettings = false } = {}) {
  statusPill.textContent = status.status;
  statusPill.dataset.status = status.status.toLowerCase();
  statusText.textContent = status.status;
  endpointText.textContent = status.endpoint;
  timeoutText.textContent = `${status.bootstrap_timeout_seconds}s`;

  if (syncSettings) {
    portInput.value = status.socks_port;
    timeoutInput.value = status.bootstrap_timeout_seconds;
    logInput.checked = status.log_connections;
  }

  const isBusy = status.status === "Running" || status.status === "Bootstrapping";
  startButton.disabled = isBusy;
  stopButton.disabled = !isBusy;
  portInput.disabled = isBusy;
  timeoutInput.disabled = isBusy;
  logInput.disabled = isBusy;

  if (status.last_error) {
    writeLog(status.last_error, "error");
  }
}

async function refreshStatus(options = {}) {
  const status = await invoke("get_status");
  renderStatus(status, options);
  writeLog(`Status refreshed: ${status.status} (${status.endpoint})`);
}

async function startSocks() {
  startButton.disabled = true;
  writeLog("Starting SOCKS proxy...");
  const status = await invoke("start_socks", {
    options: {
      socks_port: Number(portInput.value),
      log_connections: logInput.checked,
      bootstrap_timeout_seconds: Number(timeoutInput.value),
    },
  });
  renderStatus(status, { syncSettings: true });
  writeLog(`SOCKS proxy running on ${status.endpoint}`);
}

async function stopSocks() {
  stopButton.disabled = true;
  writeLog("Stopping SOCKS proxy...");
  const status = await invoke("stop_socks");
  renderStatus(status);
  writeLog("SOCKS proxy stopped.");
}

async function bindBackendLogs() {
  await listen("proxy-log", (event) => {
    const payload = event.payload;
    writeLog(payload.message, payload.level);
  });
}

startButton.addEventListener("click", () => {
  startSocks().catch((error) => {
    writeLog(String(error), "error");
    refreshStatus().catch(() => {});
  });
});

stopButton.addEventListener("click", () => {
  stopSocks().catch((error) => {
    writeLog(String(error), "error");
    refreshStatus().catch(() => {});
  });
});

refreshButton.addEventListener("click", () => {
  refreshStatus().catch((error) => writeLog(String(error), "error"));
});

clearLogButton.addEventListener("click", () => {
  messageLog.textContent = "Ready.";
});

bindBackendLogs().catch((error) => writeLog(String(error), "error"));
refreshStatus({ syncSettings: true }).catch((error) => writeLog(String(error), "error"));

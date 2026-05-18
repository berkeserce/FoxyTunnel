import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

const statusPill = document.querySelector("#status-pill");
const statusText = document.querySelector("#status-text");
const endpointText = document.querySelector("#endpoint-text");
const timeoutText = document.querySelector("#timeout-text");
const messageLog = document.querySelector("#message-log");
const startButton = document.querySelector("#start-button");
const stopButton = document.querySelector("#stop-button");
const refreshButton = document.querySelector("#refresh-button");
const portInput = document.querySelector("#port-input");
const timeoutInput = document.querySelector("#timeout-input");
const logInput = document.querySelector("#log-input");

function writeLog(message) {
  const time = new Date().toLocaleTimeString();
  messageLog.textContent = `[${time}] ${message}\n${messageLog.textContent}`;
}

function renderStatus(status) {
  statusPill.textContent = status.status;
  statusPill.dataset.status = status.status.toLowerCase();
  statusText.textContent = status.status;
  endpointText.textContent = status.endpoint;
  timeoutText.textContent = `${status.bootstrap_timeout_seconds}s`;
  portInput.value = status.socks_port;
  timeoutInput.value = status.bootstrap_timeout_seconds;
  logInput.checked = status.log_connections;

  const isBusy = status.status === "Running" || status.status === "Bootstrapping";
  startButton.disabled = isBusy;
  stopButton.disabled = !isBusy;
  portInput.disabled = isBusy;
  timeoutInput.disabled = isBusy;
  logInput.disabled = isBusy;

  if (status.last_error) {
    writeLog(status.last_error);
  }
}

async function refreshStatus() {
  const status = await invoke("get_status");
  renderStatus(status);
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
  renderStatus(status);
  writeLog(`SOCKS proxy listening on ${status.endpoint}`);
}

async function stopSocks() {
  stopButton.disabled = true;
  writeLog("Stopping SOCKS proxy...");
  const status = await invoke("stop_socks");
  renderStatus(status);
  writeLog("SOCKS proxy stopped.");
}

startButton.addEventListener("click", () => {
  startSocks().catch((error) => {
    writeLog(String(error));
    refreshStatus().catch(() => {});
  });
});

stopButton.addEventListener("click", () => {
  stopSocks().catch((error) => {
    writeLog(String(error));
    refreshStatus().catch(() => {});
  });
});

refreshButton.addEventListener("click", () => {
  refreshStatus().catch((error) => writeLog(String(error)));
});

refreshStatus().catch((error) => writeLog(String(error)));

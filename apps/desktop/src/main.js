import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

const statusPill = document.querySelector("#status-pill");
const endpointText = document.querySelector("#endpoint-text");
const messageLog = document.querySelector("#message-log");
const actionButton = document.querySelector("#action-button");
const copyEndpointButton = document.querySelector("#copy-endpoint-button");
const testTorButton = document.querySelector("#test-tor-button");
const torTestText = document.querySelector("#tor-test-text");
const refreshButton = document.querySelector("#refresh-button");
const clearLogButton = document.querySelector("#clear-log-button");
const closeButton = document.querySelector("#close-button");
const portInput = document.querySelector("#port-input");
const timeoutInput = document.querySelector("#timeout-input");
const logInput = document.querySelector("#log-input");

const MAX_VISIBLE_LOG_LINES = 120;

let lastLogSequence = 0;
let pollTimer = undefined;
let settingsSaveTimer = undefined;
let currentStatus = "Stopped";
let actionInFlight = false;
let torTestInFlight = false;

const STATUS_LABELS = {
  Bootstrapping: "Starting",
  Error: "Error",
  Running: "Running",
  Stopped: "Idle",
};

function writeLog(message, level = "info") {
  const time = new Date().toLocaleTimeString();
  const prefix = level === "error" ? "ERROR" : level.toUpperCase();
  const line = `[${time}] ${prefix}: ${message}`;
  const existing = messageLog.textContent.trim() === "Ready." ? "" : messageLog.textContent.trimEnd();
  const lines = `${existing}${existing ? "\n" : ""}${line}`.split("\n");

  messageLog.textContent = lines.slice(-MAX_VISIBLE_LOG_LINES).join("\n");
  messageLog.scrollTop = messageLog.scrollHeight;
}

function appendBackendLog(entry) {
  if (entry.sequence <= lastLogSequence) {
    return;
  }

  lastLogSequence = entry.sequence;
  writeLog(entry.message, entry.level);
}

function isEditableStatus(status) {
  return status === "Stopped" || status === "Error";
}

function renderAction(status) {
  const isBootstrapping = status === "Bootstrapping";
  const isRunning = status === "Running";

  actionButton.dataset.mode = isRunning ? "stop" : "start";

  if (isBootstrapping) {
    actionButton.textContent = "Bootstrapping...";
  } else if (isRunning) {
    actionButton.textContent = "Stop";
  } else {
    actionButton.textContent = "Start";
  }

  actionButton.disabled = actionInFlight || isBootstrapping;
}

function renderTorTest(status) {
  testTorButton.disabled = torTestInFlight || status !== "Running";
  testTorButton.textContent = torTestInFlight ? "Testing..." : "Test Tor";
}

function renderStatus(status, { syncSettings = false } = {}) {
  currentStatus = status.status;
  statusPill.textContent = STATUS_LABELS[status.status] ?? status.status;
  statusPill.dataset.status = status.status.toLowerCase();
  endpointText.textContent = status.endpoint;

  if (syncSettings) {
    portInput.value = status.socks_port;
    timeoutInput.value = status.bootstrap_timeout_seconds;
    logInput.checked = status.log_connections;
  }

  const canEdit = isEditableStatus(status.status) && !actionInFlight;
  portInput.disabled = !canEdit;
  timeoutInput.disabled = !canEdit;
  logInput.disabled = !canEdit;
  renderAction(status.status);
  renderTorTest(status.status);

  if (status.last_error) {
    writeLog(status.last_error, "error");
  }
}

async function refreshStatus(options = {}) {
  const status = await invoke("get_status");
  renderStatus(status, options);
  await syncActivityLogs();
  writeLog(`Status refreshed: ${status.status} (${status.endpoint})`);
}

function readOptions() {
  return {
    socks_port: Number(portInput.value),
    log_connections: logInput.checked,
    bootstrap_timeout_seconds: Number(timeoutInput.value),
  };
}

function scheduleSettingsSave() {
  if (!isEditableStatus(currentStatus) || actionInFlight) {
    return;
  }

  clearTimeout(settingsSaveTimer);
  settingsSaveTimer = setTimeout(() => {
    saveSettings().catch((error) => writeLog(String(error), "error"));
  }, 450);
}

async function saveSettings() {
  if (!isEditableStatus(currentStatus) || actionInFlight) {
    return;
  }

  const status = await invoke("save_settings", {
    options: readOptions(),
  });
  renderStatus(status, { syncSettings: true });
}

async function syncActivityLogs() {
  const logs = await invoke("get_activity_logs");
  for (const entry of logs.entries) {
    appendBackendLog(entry);
  }
}

async function startSocks() {
  actionInFlight = true;
  renderAction(currentStatus);
  writeLog("Starting SOCKS proxy...");

  try {
    const status = await invoke("start_socks", {
      options: readOptions(),
    });
    actionInFlight = false;
    renderStatus(status, { syncSettings: true });
    writeLog(`SOCKS proxy running on ${status.endpoint}`);
  } catch (error) {
    actionInFlight = false;
    throw error;
  }
}

async function stopSocks() {
  actionInFlight = true;
  renderAction(currentStatus);
  writeLog("Stopping SOCKS proxy...");

  try {
    const status = await invoke("stop_socks");
    actionInFlight = false;
    renderStatus(status);
    writeLog("SOCKS proxy stopped.");
  } catch (error) {
    actionInFlight = false;
    throw error;
  }
}

async function handlePrimaryAction() {
  if (currentStatus === "Running") {
    await stopSocks();
  } else if (isEditableStatus(currentStatus)) {
    await startSocks();
  }
}

async function copyEndpoint() {
  const endpoint = endpointText.textContent.trim();

  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(endpoint);
  } else {
    copyTextFallback(endpoint);
  }

  writeLog("Endpoint copied.");
}

function copyTextFallback(text) {
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

async function testTorConnection() {
  if (currentStatus !== "Running") {
    torTestText.textContent = "Start proxy first";
    torTestText.dataset.status = "unavailable";
    return;
  }

  torTestInFlight = true;
  renderTorTest(currentStatus);
  torTestText.textContent = "Testing...";
  torTestText.dataset.status = "";

  try {
    const result = await invoke("test_tor_connection");
    torTestText.textContent =
      result.status === "tor" && result.ip ? `IP: ${result.ip}` : result.message;
    torTestText.title = result.message;
    torTestText.dataset.status = result.status;
  } finally {
    torTestInFlight = false;
    renderTorTest(currentStatus);
  }
}

async function bindBackendLogs() {
  await listen("proxy-log", (event) => {
    appendBackendLog(event.payload);
  });
}

function startLogPolling() {
  clearInterval(pollTimer);
  pollTimer = setInterval(() => {
    syncActivityLogs().catch((error) => writeLog(String(error), "error"));
  }, 1500);
}

actionButton.addEventListener("click", () => {
  handlePrimaryAction().catch((error) => {
    writeLog(String(error), "error");
    refreshStatus().catch(() => {});
  });
});

copyEndpointButton.addEventListener("click", () => {
  copyEndpoint().catch((error) => writeLog(String(error), "error"));
});

testTorButton.addEventListener("click", () => {
  testTorConnection().catch((error) => {
    torTestText.textContent = String(error);
    torTestText.dataset.status = "unavailable";
    writeLog(String(error), "error");
  });
});

refreshButton.addEventListener("click", () => {
  refreshStatus().catch((error) => writeLog(String(error), "error"));
});

clearLogButton.addEventListener("click", async () => {
  await invoke("clear_activity_logs");
  lastLogSequence = 0;
  messageLog.textContent = "Ready.";
});

closeButton.addEventListener("click", () => {
  invoke("hide_panel_window").catch((error) => writeLog(String(error), "error"));
});

portInput.addEventListener("input", scheduleSettingsSave);
timeoutInput.addEventListener("input", scheduleSettingsSave);
logInput.addEventListener("change", scheduleSettingsSave);

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    invoke("hide_panel_window").catch((error) => writeLog(String(error), "error"));
  }
});

bindBackendLogs().catch((error) => writeLog(String(error), "error"));
startLogPolling();
refreshStatus({ syncSettings: true }).catch((error) => writeLog(String(error), "error"));

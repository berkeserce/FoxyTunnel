//! `FoxyTunnel` desktop application entry point.

mod http_proxy;
mod system_proxy;

use foxytunnel_core::{ConfigError, FoxyTunnelConfig, RoutingMode, SocksServerEvent, TorService};
use http_proxy::{HttpProxyBridge, HttpProxyConfig, HttpProxyEvent};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State, WebviewWindow, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Mutex, oneshot};

const MAX_LOG_LINES: usize = 500;
const MAIN_WINDOW_LABEL: &str = "main";
const APP_ICON_BYTES: &[u8] = include_bytes!("../icons/new-icon-96.png");
const SUPPORTED_EXIT_COUNTRIES: &[&str] = &["TR", "DE", "NL", "FR", "GB", "US", "CA", "SE"];

type TaskHandle = tauri::async_runtime::JoinHandle<()>;
type RuntimeTaskHandle = tauri::async_runtime::JoinHandle<Result<(), String>>;
type ReadySignal = Arc<StdMutex<Option<oneshot::Sender<String>>>>;

#[derive(Default)]
struct AppState {
    config: Mutex<FoxyTunnelConfig>,
    config_path: StdMutex<Option<PathBuf>>,
    log_path: StdMutex<Option<PathBuf>>,
    proxy: Mutex<ProxyState>,
    system_proxy: StdMutex<SystemProxyRuntime>,
    logs: StdMutex<VecDeque<LogDto>>,
}

#[derive(Default)]
struct ProxyState {
    status: ProxyStatus,
    handle: Option<TaskHandle>,
    last_error: Option<String>,
}

#[derive(Default)]
struct SystemProxyRuntime {
    active: bool,
    last_error: Option<String>,
}

struct ProxyTasks {
    socks: Option<RuntimeTaskHandle>,
    http: Option<RuntimeTaskHandle>,
}

impl Drop for ProxyTasks {
    fn drop(&mut self) {
        if let Some(handle) = &self.socks {
            handle.abort();
        }
        if let Some(handle) = &self.http {
            handle.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
enum ProxyStatus {
    #[default]
    Stopped,
    Bootstrapping,
    Running,
    Error,
}

#[derive(Serialize)]
struct StatusDto {
    status: ProxyStatus,
    endpoint: String,
    socks_port: u16,
    routing_mode: RoutingMode,
    log_connections: bool,
    exit_country: Option<String>,
    bootstrap_timeout_seconds: u64,
    last_error: Option<String>,
    system_proxy: system_proxy::SystemProxyStatus,
}

#[derive(Clone, Serialize)]
struct LogDto {
    sequence: u64,
    level: &'static str,
    message: String,
}

#[derive(Serialize)]
struct LogsDto {
    entries: Vec<LogDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TorCheckStatus {
    Tor,
    NotTor,
    Unavailable,
}

#[derive(Serialize)]
struct TorCheckDto {
    status: TorCheckStatus,
    is_tor: bool,
    ip: Option<String>,
    latency_ms: Option<u64>,
    message: String,
}

#[derive(Deserialize)]
struct TorCheckResponse {
    #[serde(rename = "IsTor")]
    is_tor: bool,
    #[serde(rename = "IP")]
    ip: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StartOptions {
    socks_port: u16,
    routing_mode: RoutingMode,
    log_connections: bool,
    exit_country: Option<String>,
    bootstrap_timeout_seconds: u64,
}

#[tauri::command]
async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> {
    Ok(status_dto(&state).await)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_activity_logs(state: State<'_, Arc<AppState>>) -> Result<LogsDto, String> {
    let logs = state
        .logs
        .lock()
        .map_err(|_| "activity log is unavailable".to_string())?;

    Ok(LogsDto {
        entries: logs.iter().cloned().collect(),
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn clear_activity_logs(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut logs = state
        .logs
        .lock()
        .map_err(|_| "activity log is unavailable".to_string())?;

    logs.clear();

    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn hide_panel_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        hide_window_to_tray(&app, &window)?;
    }

    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn open_log_folder(app: tauri::AppHandle) -> Result<String, String> {
    let path = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("failed to locate app log directory: {error}"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create log directory: {error}"))?;
    open_path(&path)?;

    Ok(path.display().to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn append_activity_log(
    state: State<'_, Arc<AppState>>,
    level: String,
    message: String,
) -> Result<(), String> {
    append_log_line_to_disk(&state, normalize_log_level(&level), &message)
}

#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    options: StartOptions,
) -> Result<StatusDto, String> {
    if !is_proxy_editable(&state).await {
        return Err("Settings can only be changed while FoxyTunnel is stopped.".to_string());
    }

    let saved_config = {
        let mut config = state.config.lock().await;
        let mut next_config = config.clone();
        apply_start_options(&mut next_config, options)?;
        persist_config(&app, &state, &next_config)?;
        *config = next_config.clone();
        next_config
    };

    emit_log(
        &app,
        &state,
        "info",
        format!("Settings saved for {}", endpoint_from_config(&saved_config)),
    );

    Ok(status_dto(&state).await)
}

#[tauri::command]
async fn reset_tor_data(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    if !is_proxy_editable(&state).await {
        return Err("Tor data can only be reset while FoxyTunnel is stopped.".to_string());
    }

    let config = state.config.lock().await.clone();
    reset_tor_data_dirs(&config)?;
    emit_log(&app, &state, "info", "Tor data reset complete");

    Ok(())
}

#[tauri::command]
async fn start_socks(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    options: StartOptions,
) -> Result<StatusDto, String> {
    if is_proxy_active(&state).await {
        return Ok(status_dto(&state).await);
    }

    {
        let mut config = state.config.lock().await;
        apply_start_options(&mut config, options)?;
    }

    let config = state.config.lock().await.clone();
    if config.routing_mode == RoutingMode::SystemProxy && !system_proxy::is_supported() {
        let status = system_proxy_status(&state);
        let error = status
            .message
            .unwrap_or_else(|| "System Proxy mode is not supported on this desktop.".to_string());
        {
            let mut proxy = state.proxy.lock().await;
            proxy.status = ProxyStatus::Error;
            proxy.last_error = Some(error.clone());
        }
        record_system_proxy_error(&state, error.clone());
        emit_log(&app, &state, "error", error.clone());
        notify_user(&app, "FoxyTunnel system proxy error", error.clone());
        return Err(error);
    }

    let already_active = {
        let mut proxy = state.proxy.lock().await;
        if proxy.status == ProxyStatus::Running || proxy.status == ProxyStatus::Bootstrapping {
            true
        } else {
            proxy.status = ProxyStatus::Bootstrapping;
            proxy.last_error = None;
            false
        }
    };

    if already_active {
        return Ok(status_dto(&state).await);
    }

    emit_log(
        &app,
        &state,
        "info",
        format!(
            "Starting SOCKS proxy on {}:{}",
            config.socks_host, config.socks_port
        ),
    );

    let result = start_proxy_runtime(&app, &state, config.clone()).await;
    let handle = match result {
        Ok(handle) => handle,
        Err(error) => {
            let mut proxy = state.proxy.lock().await;
            proxy.status = ProxyStatus::Error;
            proxy.last_error = Some(error.clone());
            emit_log(&app, &state, "error", error.clone());
            notify_user(&app, "FoxyTunnel error", error.clone());
            return Err(error);
        }
    };

    if config.routing_mode == RoutingMode::SystemProxy
        && let Err(error) = apply_system_proxy_for_config(&app, &state, &config)
    {
        handle.abort();
        let mut proxy = state.proxy.lock().await;
        proxy.status = ProxyStatus::Error;
        proxy.handle = None;
        proxy.last_error = Some(error.clone());
        record_system_proxy_error(&state, error.clone());
        emit_log(&app, &state, "error", error.clone());
        notify_user(&app, "FoxyTunnel system proxy error", error.clone());
        return Err(error);
    }

    let mut proxy = state.proxy.lock().await;
    proxy.status = ProxyStatus::Running;
    proxy.handle = Some(handle);
    proxy.last_error = None;
    if let Err(error) = persist_config(&app, &state, &config) {
        emit_log(&app, &state, "error", error);
    }

    notify_user(
        &app,
        "FoxyTunnel is running",
        running_notification_body(&config),
    );

    Ok(status_from_parts(
        &config,
        &proxy,
        system_proxy_status(&state),
    ))
}

#[tauri::command]
async fn test_tor_connection(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<TorCheckDto, String> {
    let (status, endpoint) = {
        let config = state.config.lock().await;
        let proxy = state.proxy.lock().await;
        (proxy.status, endpoint_from_config(&config))
    };

    if status != ProxyStatus::Running {
        let result = TorCheckDto::unavailable("Start the SOCKS proxy before testing Tor.");
        emit_log(&app, &state, "error", result.message.clone());
        return Ok(result);
    }

    emit_log(&app, &state, "info", "Testing Tor connection");
    let result = match check_tor_via_socks(&endpoint).await {
        Ok(result) => result,
        Err(error) => TorCheckDto::unavailable(format!("Tor check unavailable: {error}")),
    };
    let level = if result.is_tor { "info" } else { "error" };
    emit_log(&app, &state, level, result.message.clone());

    Ok(result)
}

async fn start_proxy_runtime(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: FoxyTunnelConfig,
) -> Result<TaskHandle, String> {
    emit_log(app, state, "info", "Creating Tor client");
    let mut service = TorService::create(config.tor_service_config())
        .await
        .map_err(|error| error.to_string())?;

    bootstrap_service(app, state, &mut service, config.bootstrap_timeout_seconds).await?;
    let (ready_tx, ready_rx) = oneshot::channel::<String>();
    let ready_signal = Arc::new(StdMutex::new(Some(ready_tx)));
    let socks_config = build_socks_config(app, state, &config, Some(ready_signal));

    emit_log(app, state, "info", "Starting local SOCKS listener");
    let socks_task: RuntimeTaskHandle = tauri::async_runtime::spawn(async move {
        service
            .run_socks_proxy(socks_config)
            .await
            .map_err(|error| error.to_string())
    });

    let ready_endpoint = match tokio::time::timeout(Duration::from_secs(5), ready_rx).await {
        Ok(Ok(endpoint)) => endpoint,
        Ok(Err(_)) => {
            socks_task.abort();
            return Err("SOCKS listener stopped before becoming ready".to_string());
        }
        Err(_) => {
            socks_task.abort();
            return Err("SOCKS listener did not become ready in time".to_string());
        }
    };
    emit_log(
        app,
        state,
        "info",
        format!("SOCKS listener confirmed on {ready_endpoint}"),
    );

    let mut socks_task = Some(socks_task);
    let http_task = if config.routing_mode == RoutingMode::SystemProxy && uses_http_proxy_bridge() {
        match start_http_proxy_bridge(app, state, &config).await {
            Ok(handle) => Some(handle),
            Err(error) => {
                if let Some(handle) = socks_task.take() {
                    handle.abort();
                }
                return Err(error);
            }
        }
    } else {
        None
    };

    let tasks = ProxyTasks {
        socks: socks_task,
        http: http_task,
    };
    let proxy_app = app.clone();
    let proxy_state = Arc::clone(state);
    let handle = tauri::async_runtime::spawn(async move {
        handle_proxy_runtime_stop(&proxy_app, &proxy_state, tasks).await;
    });

    Ok(handle)
}

async fn start_http_proxy_bridge(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: &FoxyTunnelConfig,
) -> Result<RuntimeTaskHandle, String> {
    let endpoints = system_proxy_endpoints_from_config(config)?;
    let (ready_tx, ready_rx) = oneshot::channel::<String>();
    let ready_signal = Arc::new(StdMutex::new(Some(ready_tx)));
    let bridge_config = build_http_proxy_config(app, state, Some(ready_signal));
    let http_authority = endpoints.http.authority();
    let socks_authority = endpoints.socks.authority();

    emit_log(
        app,
        state,
        "info",
        format!("Starting local HTTP proxy bridge on {http_authority}"),
    );

    let bridge = HttpProxyBridge::new(endpoints.http, endpoints.socks).with_config(bridge_config);
    let bridge_task: RuntimeTaskHandle = tauri::async_runtime::spawn(async move {
        bridge.run().await.map_err(|error| error.to_string())
    });

    let ready_endpoint = match tokio::time::timeout(Duration::from_secs(5), ready_rx).await {
        Ok(Ok(endpoint)) => endpoint,
        Ok(Err(_)) => return Err(task_startup_error("HTTP proxy bridge", bridge_task).await),
        Err(_) => {
            bridge_task.abort();
            return Err("HTTP proxy bridge did not become ready in time".to_string());
        }
    };
    emit_log(
        app,
        state,
        "info",
        format!(
            "HTTP proxy bridge confirmed on {ready_endpoint}; forwarding to SOCKS {socks_authority}"
        ),
    );

    Ok(bridge_task)
}

async fn task_startup_error(label: &str, handle: RuntimeTaskHandle) -> String {
    match handle.await {
        Ok(Err(error)) => format!("{label} stopped before becoming ready: {error}"),
        Ok(Ok(())) => format!("{label} stopped before becoming ready"),
        Err(error) => format!("{label} task failed before becoming ready: {error}"),
    }
}

async fn bootstrap_service(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    service: &mut TorService,
    timeout_seconds: u64,
) -> Result<(), String> {
    let timeout = Duration::from_secs(timeout_seconds);
    emit_log(
        app,
        state,
        "info",
        format!("Bootstrapping Tor with {}s timeout", timeout.as_secs()),
    );

    tokio::time::timeout(timeout, service.bootstrap())
        .await
        .map_err(|_| {
            format!(
                "Tor bootstrap timed out after {} seconds",
                timeout.as_secs()
            )
        })?
        .map_err(|error| error.to_string())?;

    emit_log(app, state, "info", "Tor bootstrap complete");

    Ok(())
}

fn build_socks_config(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: &FoxyTunnelConfig,
    ready_signal: Option<ReadySignal>,
) -> foxytunnel_core::SocksServerConfig {
    let mut socks_config = config.socks_server_config();
    let app = app.clone();
    let state = Arc::clone(state);
    socks_config.event_sink = Some(Arc::new(move |event| {
        let (level, message) = match event {
            SocksServerEvent::Listening(endpoint) => {
                if let Some(ready_signal) = &ready_signal
                    && let Ok(mut ready_signal) = ready_signal.lock()
                    && let Some(sender) = ready_signal.take()
                {
                    let _ = sender.send(endpoint.clone());
                }
                ("info", format!("SOCKS listener ready on {endpoint}"))
            }
            SocksServerEvent::Connect(target) => ("info", format!("SOCKS CONNECT {target}")),
            SocksServerEvent::ConnectionFailed(message) => ("error", message),
        };

        emit_log(&app, &state, level, message);
    }));

    socks_config
}

fn build_http_proxy_config(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    ready_signal: Option<ReadySignal>,
) -> HttpProxyConfig {
    let app = app.clone();
    let state = Arc::clone(state);

    HttpProxyConfig {
        event_sink: Some(Arc::new(move |event| {
            let (level, message) = match event {
                HttpProxyEvent::Listening(endpoint) => {
                    if let Some(ready_signal) = &ready_signal
                        && let Ok(mut ready_signal) = ready_signal.lock()
                        && let Some(sender) = ready_signal.take()
                    {
                        let _ = sender.send(endpoint.clone());
                    }
                    ("info", format!("HTTP proxy bridge ready on {endpoint}"))
                }
                HttpProxyEvent::ConnectionFailed(message) => ("error", message),
            };

            emit_log(&app, &state, level, message);
        })),
    }
}

async fn handle_proxy_runtime_stop(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    tasks: ProxyTasks,
) {
    let Some(error) = tasks.wait().await else {
        return;
    };

    if let Err(restore_error) = restore_system_proxy_for_state(app, state, false) {
        emit_log(app, state, "error", restore_error.clone());
        notify_user(app, "FoxyTunnel system proxy restore failed", restore_error);
    }
    {
        let mut proxy = state.proxy.lock().await;
        proxy.status = ProxyStatus::Error;
        proxy.last_error = Some(error.clone());
        proxy.handle = None;
    }
    emit_log(app, state, "error", error.clone());
    notify_user(app, "FoxyTunnel error", error);
}

impl ProxyTasks {
    async fn wait(mut self) -> Option<String> {
        match (self.socks.take(), self.http.take()) {
            (Some(socks), Some(http)) => {
                let mut socks_task = socks;
                let mut http_task = http;
                tokio::select! {
                    result = &mut socks_task => {
                        self.http = Some(http_task);
                        runtime_task_error("SOCKS proxy", result)
                    }
                    result = &mut http_task => {
                        self.socks = Some(socks_task);
                        runtime_task_error("HTTP proxy bridge", result)
                    }
                }
            }
            (Some(socks), None) => runtime_task_error("SOCKS proxy", socks.await),
            (None, Some(http)) => runtime_task_error("HTTP proxy bridge", http.await),
            (None, None) => None,
        }
    }
}

fn runtime_task_error(
    label: &str,
    result: Result<Result<(), String>, tauri::Error>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => Some(format!("{label} stopped")),
        Ok(Err(error)) => Some(format!("{label} stopped: {error}")),
        Err(tauri::Error::JoinError(error)) if error.is_cancelled() => None,
        Err(error) => Some(format!("{label} task failed: {error}")),
    }
}

#[tauri::command]
async fn stop_socks(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<StatusDto, String> {
    let restore_result = restore_system_proxy_for_state(&app, &state, false);
    {
        let mut proxy = state.proxy.lock().await;
        if let Some(handle) = proxy.handle.take() {
            handle.abort();
        }
        if let Err(error) = &restore_result {
            proxy.status = ProxyStatus::Error;
            proxy.last_error = Some(error.clone());
        } else {
            proxy.status = ProxyStatus::Stopped;
            proxy.last_error = None;
        }
    }

    if let Err(error) = restore_result {
        record_system_proxy_error(&state, error.clone());
        emit_log(&app, &state, "error", error.clone());
        notify_user(
            &app,
            "FoxyTunnel system proxy restore failed",
            error.clone(),
        );
        return Err(error);
    }

    emit_log(&app, &state, "info", "SOCKS proxy stopped");
    notify_user(&app, "FoxyTunnel stopped", "SOCKS proxy is stopped.");

    Ok(status_dto(&state).await)
}

async fn check_tor_via_socks(endpoint: &str) -> Result<TorCheckDto, String> {
    let proxy = reqwest::Proxy::all(format!("socks5h://{endpoint}"))
        .map_err(|error| format!("failed to configure SOCKS proxy: {error}"))?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("failed to create Tor check client: {error}"))?;
    let start = Instant::now();
    let response = client
        .get("https://check.torproject.org/api/ip")
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Tor Check returned an error: {error}"))?
        .json::<TorCheckResponse>()
        .await
        .map_err(|error| format!("failed to read Tor Check response: {error}"))?;

    Ok(tor_check_from_response(
        response,
        Some(duration_millis(start.elapsed())),
    ))
}

fn tor_check_from_response(response: TorCheckResponse, latency_ms: Option<u64>) -> TorCheckDto {
    if response.is_tor {
        let suffix = response
            .ip
            .as_deref()
            .map_or_else(String::new, |ip| format!(" Exit IP: {ip}."));
        TorCheckDto {
            status: TorCheckStatus::Tor,
            is_tor: true,
            ip: response.ip,
            latency_ms,
            message: format!("Tor connection verified.{suffix}"),
        }
    } else {
        TorCheckDto {
            status: TorCheckStatus::NotTor,
            is_tor: false,
            ip: response.ip,
            latency_ms,
            message: "Connection reached Tor Check but was not identified as Tor.".to_string(),
        }
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl TorCheckDto {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: TorCheckStatus::Unavailable,
            is_tor: false,
            ip: None,
            latency_ms: None,
            message: message.into(),
        }
    }
}

fn emit_log(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    level: &'static str,
    message: impl Into<String>,
) {
    let entry = push_log(state, level, message.into());
    let _ = app.emit("proxy-log", entry);
}

fn notify_user(app: &tauri::AppHandle, title: impl Into<String>, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title.into())
        .body(body.into())
        .show();
}

fn push_log(state: &Arc<AppState>, level: &'static str, message: String) -> LogDto {
    let entry = {
        let Ok(mut logs) = state.logs.lock() else {
            let entry = LogDto {
                sequence: 0,
                level,
                message,
            };
            let _ = append_log_line_to_disk(state, entry.level, &entry.message);
            return entry;
        };

        let sequence = logs.back().map_or(1, |entry| entry.sequence + 1);
        let entry = LogDto {
            sequence,
            level,
            message,
        };

        logs.push_back(entry.clone());
        while logs.len() > MAX_LOG_LINES {
            logs.pop_front();
        }

        entry
    };

    let _ = append_log_line_to_disk(state, entry.level, &entry.message);

    entry
}

fn append_log_line_to_disk(
    state: &Arc<AppState>,
    level: &'static str,
    message: &str,
) -> Result<(), String> {
    let Some(path) = state
        .log_path
        .lock()
        .map_err(|_| "log path is unavailable".to_string())?
        .clone()
    else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create log directory: {error}"))?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("failed to open log file: {error}"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let message = sanitize_log_message(message);

    writeln!(file, "[{timestamp}] {}: {message}", level.to_uppercase())
        .map_err(|error| format!("failed to write log file: {error}"))
}

fn sanitize_log_message(message: &str) -> String {
    message.replace(['\r', '\n'], " ")
}

fn normalize_log_level(level: &str) -> &'static str {
    if level.eq_ignore_ascii_case("error") {
        "error"
    } else {
        "info"
    }
}

fn init_log_file(app: &tauri::AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    let path = log_file_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create log directory: {error}"))?;
    }

    *state
        .log_path
        .lock()
        .map_err(|_| "log path is unavailable".to_string())? = Some(path);

    Ok(())
}

fn log_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let process_id = std::process::id();

    app.path()
        .app_log_dir()
        .map(|path| path.join(format!("foxytunnel-{stamp}-{process_id}.log")))
        .map_err(|error| format!("failed to locate app log directory: {error}"))
}

fn open_path(path: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "windows") {
        Command::new("explorer").arg(path).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()
    } else {
        Command::new("xdg-open").arg(path).spawn()
    };

    result
        .map(|_| ())
        .map_err(|error| format!("failed to open {}: {error}", path.display()))
}

async fn status_dto(state: &Arc<AppState>) -> StatusDto {
    let config = state.config.lock().await.clone();
    let proxy = state.proxy.lock().await;
    let system_proxy = system_proxy_status(state);

    status_from_parts(&config, &proxy, system_proxy)
}

async fn is_proxy_active(state: &Arc<AppState>) -> bool {
    let proxy = state.proxy.lock().await;

    proxy.status == ProxyStatus::Running || proxy.status == ProxyStatus::Bootstrapping
}

async fn is_proxy_editable(state: &Arc<AppState>) -> bool {
    let proxy = state.proxy.lock().await;

    matches!(proxy.status, ProxyStatus::Stopped | ProxyStatus::Error)
}

fn apply_start_options(config: &mut FoxyTunnelConfig, options: StartOptions) -> Result<(), String> {
    if options.socks_port == 0 {
        return Err("SOCKS port must be between 1 and 65535".to_string());
    }
    if !(10..=600).contains(&options.bootstrap_timeout_seconds) {
        return Err("Bootstrap timeout must be between 10 and 600 seconds".to_string());
    }
    if options.routing_mode == RoutingMode::SystemProxy && uses_http_proxy_bridge() {
        http_proxy_port(options.socks_port)?;
    }

    config.socks_port = options.socks_port;
    config.routing_mode = options.routing_mode;
    config.log_connections = options.log_connections;
    config.exit_country = normalize_exit_country(options.exit_country)?;
    config.bootstrap_timeout_seconds = options.bootstrap_timeout_seconds;

    Ok(())
}

fn normalize_exit_country(country: Option<String>) -> Result<Option<String>, String> {
    let Some(country) = country else {
        return Ok(None);
    };
    let code = country.trim().to_ascii_uppercase();

    if code.is_empty() || code == "AUTO" {
        return Ok(None);
    }

    if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err("Exit country must be an ISO alpha-2 country code.".to_string());
    }

    if !SUPPORTED_EXIT_COUNTRIES.contains(&code.as_str()) {
        return Err(format!(
            "Unsupported exit country {code}. Supported countries: {}",
            SUPPORTED_EXIT_COUNTRIES.join(", ")
        ));
    }

    Ok(Some(code))
}

fn load_persisted_config(app: &tauri::AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    let path = config_file_path(app)?;
    set_config_path(state, path.clone())?;

    match FoxyTunnelConfig::load(&path) {
        Ok(config) => {
            tauri::async_runtime::block_on(async {
                *state.config.lock().await = config;
            });
            Ok(())
        }
        Err(ConfigError::Read(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn persist_config(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: &FoxyTunnelConfig,
) -> Result<(), String> {
    let path = ensure_config_path(app, state)?;
    write_config_to_file(&path, config)
}

fn ensure_config_path(app: &tauri::AppHandle, state: &Arc<AppState>) -> Result<PathBuf, String> {
    if let Some(path) = state
        .config_path
        .lock()
        .map_err(|_| "config path is unavailable".to_string())?
        .clone()
    {
        return Ok(path);
    }

    let path = config_file_path(app)?;
    set_config_path(state, path.clone())?;

    Ok(path)
}

fn set_config_path(state: &Arc<AppState>, path: PathBuf) -> Result<(), String> {
    *state
        .config_path
        .lock()
        .map_err(|_| "config path is unavailable".to_string())? = Some(path);

    Ok(())
}

fn config_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| config_file_path_from_dir(&path))
        .map_err(|error| format!("failed to locate app config directory: {error}"))
}

fn config_file_path_from_dir(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

fn write_config_to_file(path: &Path, config: &FoxyTunnelConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create config directory: {error}"))?;
    }

    let contents = config.to_toml_string().map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| format!("failed to write config: {error}"))
}

fn reset_tor_data_dirs(config: &FoxyTunnelConfig) -> Result<(), String> {
    remove_dir_if_exists(&config.arti_state_dir)?;

    if config.arti_cache_dir != config.arti_state_dir {
        remove_dir_if_exists(&config.arti_cache_dir)?;
    }

    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

fn endpoint_from_config(config: &FoxyTunnelConfig) -> String {
    format!("{}:{}", config.socks_host, config.socks_port)
}

fn status_from_parts(
    config: &FoxyTunnelConfig,
    proxy: &ProxyState,
    system_proxy: system_proxy::SystemProxyStatus,
) -> StatusDto {
    StatusDto {
        status: proxy.status,
        endpoint: endpoint_from_config(config),
        socks_port: config.socks_port,
        routing_mode: config.routing_mode,
        log_connections: config.log_connections,
        exit_country: config.exit_country.clone(),
        bootstrap_timeout_seconds: config.bootstrap_timeout_seconds,
        last_error: proxy.last_error.clone(),
        system_proxy,
    }
}

fn running_notification_body(config: &FoxyTunnelConfig) -> String {
    match config.routing_mode {
        RoutingMode::SocksOnly => format!(
            "SOCKS proxy is listening on {}:{}",
            config.socks_host, config.socks_port
        ),
        RoutingMode::SystemProxy => {
            "System proxy is routed through FoxyTunnel for proxy-aware apps.".to_string()
        }
    }
}

fn system_proxy_status(state: &Arc<AppState>) -> system_proxy::SystemProxyStatus {
    match state.system_proxy.lock() {
        Ok(runtime) => system_proxy::platform_status(runtime.active, runtime.last_error.clone()),
        Err(_) => system_proxy::platform_status(
            false,
            Some("system proxy state is unavailable".to_string()),
        ),
    }
}

fn system_proxy_endpoints_from_config(
    config: &FoxyTunnelConfig,
) -> Result<system_proxy::SystemProxyEndpoints, String> {
    let socks = system_proxy::ProxyEndpoint {
        host: config.socks_host.clone(),
        port: config.socks_port,
    };
    let http = if uses_http_proxy_bridge() {
        system_proxy::ProxyEndpoint {
            host: config.socks_host.clone(),
            port: http_proxy_port(config.socks_port)?,
        }
    } else {
        socks.clone()
    };

    Ok(system_proxy::SystemProxyEndpoints { socks, http })
}

fn uses_http_proxy_bridge() -> bool {
    cfg!(target_os = "windows")
}

fn http_proxy_port(socks_port: u16) -> Result<u16, String> {
    socks_port.checked_add(1).ok_or_else(|| {
        "System Proxy mode needs one free local port after the SOCKS port.".to_string()
    })
}

fn system_proxy_snapshot_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("system-proxy-snapshot.json"))
        .map_err(|error| format!("failed to locate app config directory: {error}"))
}

fn apply_system_proxy_for_config(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: &FoxyTunnelConfig,
) -> Result<(), String> {
    let snapshot_path = system_proxy_snapshot_path(app)?;
    let endpoints = system_proxy_endpoints_from_config(config)?;
    let status = system_proxy::apply_system_proxy(&endpoints, &snapshot_path)?;
    {
        let mut runtime = state
            .system_proxy
            .lock()
            .map_err(|_| "system proxy state is unavailable".to_string())?;
        runtime.active = true;
        runtime.last_error = None;
    }
    emit_log(
        app,
        state,
        "info",
        system_proxy_enabled_message(&status, &endpoints),
    );

    Ok(())
}

fn system_proxy_enabled_message(
    status: &system_proxy::SystemProxyStatus,
    endpoints: &system_proxy::SystemProxyEndpoints,
) -> String {
    if uses_http_proxy_bridge() {
        format!(
            "System proxy enabled via {} (HTTP {} -> SOCKS {})",
            status.backend,
            endpoints.http.authority(),
            endpoints.socks.authority()
        )
    } else {
        format!(
            "System proxy enabled via {} (SOCKS {})",
            status.backend,
            endpoints.socks.authority()
        )
    }
}

fn restore_system_proxy_for_state(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    only_if_owned: bool,
) -> Result<bool, String> {
    let snapshot_path = system_proxy_snapshot_path(app)?;
    let restored = if only_if_owned {
        system_proxy::restore_system_proxy_if_owned(&snapshot_path)?
    } else {
        system_proxy::restore_system_proxy(&snapshot_path)?
    };

    let mut runtime = state
        .system_proxy
        .lock()
        .map_err(|_| "system proxy state is unavailable".to_string())?;
    runtime.active = false;
    runtime.last_error = None;

    Ok(restored)
}

fn record_system_proxy_error(state: &Arc<AppState>, error: String) {
    if let Ok(mut runtime) = state.system_proxy.lock() {
        runtime.last_error = Some(error);
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(AppState::default()))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let state = Arc::clone(app.state::<Arc<AppState>>().inner());
            if let Err(error) = init_log_file(app.handle(), &state) {
                emit_log(app.handle(), &state, "error", error);
            }
            emit_log(app.handle(), &state, "info", "FoxyTunnel session started");
            match restore_system_proxy_for_state(app.handle(), &state, true) {
                Ok(true) => emit_log(
                    app.handle(),
                    &state,
                    "info",
                    "Restored stale system proxy settings from previous session",
                ),
                Ok(false) => {}
                Err(error) => emit_log(app.handle(), &state, "error", error),
            }
            if let Err(error) = load_persisted_config(app.handle(), &state) {
                emit_log(app.handle(), &state, "error", error);
            }
            setup_tray(app.handle())?;
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                configure_panel_window(&window)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                if window.hide().is_ok() {
                    notify_hidden_to_tray(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            append_activity_log,
            clear_activity_logs,
            get_activity_logs,
            get_status,
            hide_panel_window,
            open_log_folder,
            reset_tor_data,
            save_settings,
            start_socks,
            stop_socks,
            test_tor_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running FoxyTunnel desktop app");
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show_panel", "Open FoxyTunnel").build(app)?;
    let hide = MenuItemBuilder::with_id("hide_panel", "Hide").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .item(&hide)
        .separator()
        .item(&quit)
        .build()?;

    let mut tray = TrayIconBuilder::with_id("foxytunnel-tray")
        .tooltip("FoxyTunnel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_panel" => show_panel(app),
            "hide_panel" => hide_panel(app),
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle());
            }
        });

    if let Ok(icon) = Image::from_bytes(APP_ICON_BYTES) {
        tray = tray.icon(icon);
    } else if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;

    Ok(())
}

fn configure_panel_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_decorations(true)?;
    window.set_resizable(false)?;
    window.set_always_on_top(false)?;
    window.set_skip_taskbar(false)?;

    Ok(())
}

fn toggle_panel(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    if window.is_minimized().is_ok_and(|minimized| minimized)
        || !window.is_visible().is_ok_and(|visible| visible)
        || !window.is_focused().is_ok_and(|focused| focused)
    {
        show_window(&window);
    } else {
        let _ = hide_window_to_tray(app, &window);
    }
}

fn show_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        show_window(&window);
    }
}

fn hide_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = hide_window_to_tray(app, &window);
    }
}

fn quit_app(app: &tauri::AppHandle) {
    let state = Arc::clone(app.state::<Arc<AppState>>().inner());
    if let Err(error) = restore_system_proxy_for_state(app, &state, false) {
        emit_log(app, &state, "error", error);
    }
    app.exit(0);
}

fn show_window(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn hide_window_to_tray(app: &tauri::AppHandle, window: &WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())?;
    notify_hidden_to_tray(app);

    Ok(())
}

fn notify_hidden_to_tray(app: &tauri::AppHandle) {
    notify_user(
        app,
        "FoxyTunnel is still running",
        "The window was hidden to tray. Use the tray icon to bring it back.",
    );
}

#[cfg(test)]
mod tests {
    use super::{
        StartOptions, TorCheckResponse, TorCheckStatus, apply_start_options,
        config_file_path_from_dir, normalize_exit_country, reset_tor_data_dirs,
        tor_check_from_response, write_config_to_file,
    };
    use foxytunnel_core::{FoxyTunnelConfig, RoutingMode};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tor_check_json_parses_verified_response() {
        let response: TorCheckResponse =
            serde_json::from_str(r#"{"IsTor":true,"IP":"185.220.101.1"}"#)
                .expect("Tor Check JSON should parse");
        let result = tor_check_from_response(response, Some(123));

        assert_eq!(result.status, TorCheckStatus::Tor);
        assert!(result.is_tor);
        assert_eq!(result.ip.as_deref(), Some("185.220.101.1"));
        assert_eq!(result.latency_ms, Some(123));
    }

    #[test]
    fn config_load_save_roundtrips() {
        let dir = unique_temp_dir();
        let path = config_file_path_from_dir(&dir);
        let config = FoxyTunnelConfig {
            socks_port: 19_051,
            routing_mode: RoutingMode::SystemProxy,
            log_connections: true,
            exit_country: Some("DE".to_string()),
            bootstrap_timeout_seconds: 90,
            ..FoxyTunnelConfig::default()
        };

        write_config_to_file(&path, &config).expect("config should save");
        let loaded = FoxyTunnelConfig::load(&path).expect("config should load");

        assert_eq!(loaded, config);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_timeout_is_rejected() {
        let mut config = FoxyTunnelConfig::default();
        let result = apply_start_options(
            &mut config,
            StartOptions {
                socks_port: 19_050,
                routing_mode: RoutingMode::SocksOnly,
                log_connections: false,
                exit_country: None,
                bootstrap_timeout_seconds: 5,
            },
        );

        assert!(result.is_err());
        assert_eq!(config.bootstrap_timeout_seconds, 120);
    }

    #[test]
    fn exit_country_is_normalized_and_validated() {
        assert_eq!(
            normalize_exit_country(Some("de".to_string())).expect("country should normalize"),
            Some("DE".to_string())
        );
        assert_eq!(
            normalize_exit_country(Some("auto".to_string())).expect("auto should clear country"),
            None
        );
        assert!(normalize_exit_country(Some("ZZ".to_string())).is_err());
        assert!(normalize_exit_country(Some("DEU".to_string())).is_err());
    }

    #[test]
    fn reset_tor_data_removes_state_and_cache_only() {
        let dir = unique_temp_dir();
        let config_path = config_file_path_from_dir(&dir);
        let state_dir = dir.join("state");
        let cache_dir = dir.join("cache");
        std::fs::create_dir_all(&state_dir).expect("state dir should be created");
        std::fs::create_dir_all(&cache_dir).expect("cache dir should be created");
        std::fs::write(state_dir.join("state.txt"), "state").expect("state file should be written");
        std::fs::write(cache_dir.join("cache.txt"), "cache").expect("cache file should be written");

        let config = FoxyTunnelConfig {
            arti_state_dir: state_dir.clone(),
            arti_cache_dir: cache_dir.clone(),
            ..FoxyTunnelConfig::default()
        };
        write_config_to_file(&config_path, &config).expect("config should save");

        reset_tor_data_dirs(&config).expect("tor data should reset");

        assert!(!state_dir.exists());
        assert!(!cache_dir.exists());
        assert!(config_path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(Path::new(&format!(
            "foxytunnel-desktop-test-{}-{stamp}",
            std::process::id()
        )))
    }
}

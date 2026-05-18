//! `FoxyTunnel` desktop application entry point.

use foxytunnel_core::{FoxyTunnelConfig, SocksServerEvent, TorService};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

const MAX_LOG_LINES: usize = 500;

type TaskHandle = tauri::async_runtime::JoinHandle<()>;

#[derive(Default)]
struct AppState {
    config: Mutex<FoxyTunnelConfig>,
    proxy: Mutex<ProxyState>,
    logs: StdMutex<VecDeque<LogDto>>,
}

#[derive(Default)]
struct ProxyState {
    status: ProxyStatus,
    handle: Option<TaskHandle>,
    event_handle: Option<TaskHandle>,
    last_error: Option<String>,
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
    log_connections: bool,
    bootstrap_timeout_seconds: u64,
    last_error: Option<String>,
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

#[derive(Debug, Clone, Copy, Deserialize)]
struct StartOptions {
    socks_port: u16,
    log_connections: bool,
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

    let config = state.config.lock().await.clone();
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

    let mut proxy = state.proxy.lock().await;
    match result {
        Ok((handle, event_handle)) => {
            proxy.status = ProxyStatus::Running;
            proxy.handle = Some(handle);
            proxy.event_handle = Some(event_handle);
            proxy.last_error = None;
        }
        Err(error) => {
            proxy.status = ProxyStatus::Error;
            proxy.last_error = Some(error.clone());
            emit_log(&app, &state, "error", error.clone());
            return Err(error);
        }
    }

    Ok(status_from_parts(&config, &proxy))
}

async fn start_proxy_runtime(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: FoxyTunnelConfig,
) -> Result<(TaskHandle, TaskHandle), String> {
    emit_log(app, state, "info", "Creating Tor client");
    let mut service = TorService::create(config.tor_service_config())
        .await
        .map_err(|error| error.to_string())?;

    bootstrap_service(app, state, &mut service, config.bootstrap_timeout_seconds).await?;
    let (event_handle, socks_config) = build_socks_event_bridge(app, state, &config);

    emit_log(app, state, "info", "Starting local SOCKS listener");
    let proxy_app = app.clone();
    let proxy_state = Arc::clone(state);
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(error) = service.run_socks_proxy(socks_config).await {
            emit_log(
                &proxy_app,
                &proxy_state,
                "error",
                format!("SOCKS proxy stopped: {error}"),
            );
        }
    });

    Ok((handle, event_handle))
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

fn build_socks_event_bridge(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    config: &FoxyTunnelConfig,
) -> (TaskHandle, foxytunnel_core::SocksServerConfig) {
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let mut socks_config = config.socks_server_config();
    socks_config.event_sender = Some(event_sender);

    let event_app = app.clone();
    let event_state = Arc::clone(state);
    let event_handle = tauri::async_runtime::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            let (level, message) = match event {
                SocksServerEvent::Listening(endpoint) => {
                    ("info", format!("SOCKS listener ready on {endpoint}"))
                }
                SocksServerEvent::Connect(target) => ("info", format!("SOCKS CONNECT {target}")),
                SocksServerEvent::ConnectionFailed(message) => ("error", message),
            };

            emit_log(&event_app, &event_state, level, message);
        }
    });

    (event_handle, socks_config)
}

#[tauri::command]
async fn stop_socks(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> {
    {
        let mut proxy = state.proxy.lock().await;
        if let Some(handle) = proxy.handle.take() {
            handle.abort();
        }
        if let Some(handle) = proxy.event_handle.take() {
            handle.abort();
        }
        proxy.status = ProxyStatus::Stopped;
        proxy.last_error = None;
    }

    Ok(status_dto(&state).await)
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

fn push_log(state: &Arc<AppState>, level: &'static str, message: String) -> LogDto {
    let Ok(mut logs) = state.logs.lock() else {
        return LogDto {
            sequence: 0,
            level,
            message,
        };
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
}

async fn status_dto(state: &Arc<AppState>) -> StatusDto {
    let config = state.config.lock().await.clone();
    let proxy = state.proxy.lock().await;

    status_from_parts(&config, &proxy)
}

async fn is_proxy_active(state: &Arc<AppState>) -> bool {
    let proxy = state.proxy.lock().await;

    proxy.status == ProxyStatus::Running || proxy.status == ProxyStatus::Bootstrapping
}

fn apply_start_options(config: &mut FoxyTunnelConfig, options: StartOptions) -> Result<(), String> {
    if options.socks_port == 0 {
        return Err("SOCKS port must be between 1 and 65535".to_string());
    }
    if !(10..=600).contains(&options.bootstrap_timeout_seconds) {
        return Err("Bootstrap timeout must be between 10 and 600 seconds".to_string());
    }

    config.socks_port = options.socks_port;
    config.log_connections = options.log_connections;
    config.bootstrap_timeout_seconds = options.bootstrap_timeout_seconds;

    Ok(())
}

fn status_from_parts(config: &FoxyTunnelConfig, proxy: &ProxyState) -> StatusDto {
    StatusDto {
        status: proxy.status,
        endpoint: format!("{}:{}", config.socks_host, config.socks_port),
        socks_port: config.socks_port,
        log_connections: config.log_connections,
        bootstrap_timeout_seconds: config.bootstrap_timeout_seconds,
        last_error: proxy.last_error.clone(),
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            clear_activity_logs,
            get_activity_logs,
            get_status,
            start_socks,
            stop_socks
        ])
        .run(tauri::generate_context!())
        .expect("error while running FoxyTunnel desktop app");
}

//! `FoxyTunnel` desktop application entry point.

use foxytunnel_core::{FoxyTunnelConfig, SocksServerEvent, TorService};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent};
use tokio::sync::Mutex;

const MAX_LOG_LINES: usize = 500;
const MAIN_WINDOW_LABEL: &str = "main";
const PANEL_MARGIN: i32 = 12;

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
        Ok(handle) => {
            proxy.status = ProxyStatus::Running;
            proxy.handle = Some(handle);
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
) -> Result<TaskHandle, String> {
    emit_log(app, state, "info", "Creating Tor client");
    let mut service = TorService::create(config.tor_service_config())
        .await
        .map_err(|error| error.to_string())?;

    bootstrap_service(app, state, &mut service, config.bootstrap_timeout_seconds).await?;
    let socks_config = build_socks_config(app, state, &config);

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

    Ok(handle)
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
) -> foxytunnel_core::SocksServerConfig {
    let mut socks_config = config.socks_server_config();
    let app = app.clone();
    let state = Arc::clone(state);
    socks_config.event_sink = Some(Arc::new(move |event| {
        let (level, message) = match event {
            SocksServerEvent::Listening(endpoint) => {
                ("info", format!("SOCKS listener ready on {endpoint}"))
            }
            SocksServerEvent::Connect(target) => ("info", format!("SOCKS CONNECT {target}")),
            SocksServerEvent::ConnectionFailed(message) => ("error", message),
        };

        emit_log(&app, &state, level, message);
    }));

    socks_config
}

#[tauri::command]
async fn stop_socks(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> {
    {
        let mut proxy = state.proxy.lock().await;
        if let Some(handle) = proxy.handle.take() {
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
        .setup(|app| {
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
                let _ = window.hide();
            }
        })
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
            "quit" => app.exit(0),
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

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;

    Ok(())
}

fn configure_panel_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_skip_taskbar(true)?;
    position_panel_window(window)?;
    window.hide()?;

    Ok(())
}

fn toggle_panel(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => {
            show_window(&window);
        }
    }
}

fn show_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        show_window(&window);
    }
}

fn hide_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

fn show_window(window: &WebviewWindow) {
    let _ = position_panel_window(window);
    let _ = window.show();
    let _ = window.set_focus();
}

fn position_panel_window(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };

    let work_area = monitor.work_area();
    let outer_size = window.outer_size()?;
    let x = work_area.position.x + PANEL_MARGIN;
    let work_area_height = i32::try_from(work_area.size.height).unwrap_or(i32::MAX);
    let window_height = i32::try_from(outer_size.height).unwrap_or(i32::MAX);
    let y = work_area.position.y + work_area_height - window_height - PANEL_MARGIN;

    window.set_position(PhysicalPosition::new(x, y))
}

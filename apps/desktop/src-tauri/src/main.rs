//! `FoxyTunnel` desktop application entry point.

use foxytunnel_core::{FoxyTunnelConfig, SocksServerEvent, TorService};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, PhysicalPosition, Rect, State, WebviewWindow, WindowEvent};
use tauri_plugin_notification::NotificationExt;
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
    last_tray_anchor: StdMutex<Option<TrayAnchor>>,
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
#[allow(clippy::needless_pass_by_value)]
fn hide_panel_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window.hide().map_err(|error| error.to_string())?;
    }

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
            notify_user(&app, "FoxyTunnel error", error.clone());
            return Err(error);
        }
    }

    notify_user(
        &app,
        "FoxyTunnel is running",
        format!(
            "SOCKS proxy is listening on {}:{}",
            config.socks_host, config.socks_port
        ),
    );

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
            let error = format!("SOCKS proxy stopped: {error}");
            {
                let mut proxy = proxy_state.proxy.lock().await;
                proxy.status = ProxyStatus::Error;
                proxy.last_error = Some(error.clone());
                proxy.handle = None;
            }
            emit_log(&proxy_app, &proxy_state, "error", error.clone());
            notify_user(&proxy_app, "FoxyTunnel error", error);
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
async fn stop_socks(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<StatusDto, String> {
    {
        let mut proxy = state.proxy.lock().await;
        if let Some(handle) = proxy.handle.take() {
            handle.abort();
        }
        proxy.status = ProxyStatus::Stopped;
        proxy.last_error = None;
    }

    emit_log(&app, &state, "info", "SOCKS proxy stopped");
    notify_user(&app, "FoxyTunnel stopped", "SOCKS proxy is stopped.");

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

fn notify_user(app: &tauri::AppHandle, title: impl Into<String>, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title.into())
        .body(body.into())
        .show();
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
        .plugin(tauri_plugin_notification::init())
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
        .invoke_handler(tauri::generate_handler![
            clear_activity_logs,
            get_activity_logs,
            get_status,
            hide_panel_window,
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
            if let Some(anchor) = tray_event_anchor(&event) {
                remember_tray_anchor(tray.app_handle(), anchor);
            }

            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), tray_event_anchor(&event));
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;

    Ok(())
}

fn configure_panel_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_decorations(false)?;
    window.set_resizable(false)?;
    window.set_always_on_top(true)?;
    window.set_skip_taskbar(true)?;
    position_panel_window(window, None)?;
    window.hide()?;

    Ok(())
}

fn toggle_panel(app: &tauri::AppHandle, anchor: Option<TrayAnchor>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => {
            show_window(&window, anchor.or_else(|| last_tray_anchor(app)));
        }
    }
}

fn show_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        show_window(&window, last_tray_anchor(app));
    }
}

fn hide_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

fn show_window(window: &WebviewWindow, anchor: Option<TrayAnchor>) {
    let _ = position_panel_window(window, anchor);
    let _ = window.show();
    let _ = window.set_focus();
}

fn position_panel_window(window: &WebviewWindow, anchor: Option<TrayAnchor>) -> tauri::Result<()> {
    let monitor = if let Some(anchor) = anchor {
        window
            .monitor_from_point(anchor.center_x(), anchor.center_y())?
            .or(window.current_monitor()?)
            .or(window.primary_monitor()?)
    } else {
        window.current_monitor()?.or(window.primary_monitor()?)
    };

    let Some(monitor) = monitor else {
        return Ok(());
    };

    let work_area = monitor.work_area();
    let outer_size = window.outer_size()?;
    let work_area_width = i32::try_from(work_area.size.width).unwrap_or(i32::MAX);
    let work_area_height = i32::try_from(work_area.size.height).unwrap_or(i32::MAX);
    let window_width = i32::try_from(outer_size.width).unwrap_or(i32::MAX);
    let window_height = i32::try_from(outer_size.height).unwrap_or(i32::MAX);
    let work_left = work_area.position.x;
    let work_top = work_area.position.y;
    let work_right = work_left.saturating_add(work_area_width);
    let work_bottom = work_top.saturating_add(work_area_height);

    let (preferred_x, preferred_y) = if let Some(anchor) = anchor {
        let x = anchor
            .center_x_i32()
            .saturating_sub(window_width.saturating_div(2));
        let mut y = work_bottom
            .saturating_sub(window_height)
            .saturating_sub(PANEL_MARGIN);

        if anchor.center_y() < f64::from(work_top.saturating_add(work_area_height / 2)) {
            y = anchor
                .y
                .saturating_add(anchor.height)
                .saturating_add(PANEL_MARGIN);
        }

        (x, y)
    } else {
        (
            work_right
                .saturating_sub(window_width)
                .saturating_sub(PANEL_MARGIN),
            work_bottom
                .saturating_sub(window_height)
                .saturating_sub(PANEL_MARGIN),
        )
    };

    let x = clamp_panel_axis(preferred_x, work_left, work_right, window_width);
    let y = clamp_panel_axis(preferred_y, work_top, work_bottom, window_height);

    window.set_position(PhysicalPosition::new(x, y))
}

fn remember_tray_anchor(app: &tauri::AppHandle, anchor: TrayAnchor) {
    let state = app.state::<Arc<AppState>>();
    if let Ok(mut last_tray_anchor) = state.last_tray_anchor.lock() {
        *last_tray_anchor = Some(anchor);
    }
}

fn last_tray_anchor(app: &tauri::AppHandle) -> Option<TrayAnchor> {
    let state = app.state::<Arc<AppState>>();
    state
        .last_tray_anchor
        .lock()
        .ok()
        .and_then(|last_tray_anchor| *last_tray_anchor)
}

fn tray_event_anchor(event: &TrayIconEvent) -> Option<TrayAnchor> {
    match event {
        TrayIconEvent::Click { position, rect, .. }
        | TrayIconEvent::DoubleClick { position, rect, .. }
        | TrayIconEvent::Enter { position, rect, .. }
        | TrayIconEvent::Move { position, rect, .. }
        | TrayIconEvent::Leave { position, rect, .. } => {
            Some(TrayAnchor::from_event(*position, *rect))
        }
        _ => None,
    }
}

fn clamp_panel_axis(preferred: i32, work_start: i32, work_end: i32, window_size: i32) -> i32 {
    let min = work_start.saturating_add(PANEL_MARGIN);
    let max = work_end
        .saturating_sub(window_size)
        .saturating_sub(PANEL_MARGIN);

    if max < min {
        work_start
    } else {
        preferred.clamp(min, max)
    }
}

#[derive(Clone, Copy)]
struct TrayAnchor {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl TrayAnchor {
    fn from_event(position: PhysicalPosition<f64>, rect: Rect) -> Self {
        let rect_position = rect.position.to_physical::<i32>(1.0);
        let rect_size = rect.size.to_physical::<u32>(1.0);
        let width = i32::try_from(rect_size.width).unwrap_or(i32::MAX);
        let height = i32::try_from(rect_size.height).unwrap_or(i32::MAX);

        if width > 0 && height > 0 {
            Self {
                x: rect_position.x,
                y: rect_position.y,
                width,
                height,
            }
        } else {
            let position = position.cast::<i32>();
            Self {
                x: position.x,
                y: position.y,
                width: 0,
                height: 0,
            }
        }
    }

    fn center_x(self) -> f64 {
        f64::from(self.x) + f64::from(self.width) / 2.0
    }

    fn center_y(self) -> f64 {
        f64::from(self.y) + f64::from(self.height) / 2.0
    }

    fn center_x_i32(self) -> i32 {
        self.x.saturating_add(self.width.saturating_div(2))
    }
}

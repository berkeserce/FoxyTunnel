//! `FoxyTunnel` desktop application entry point.

use foxytunnel_core::{ConfigError, FoxyTunnelConfig, SocksServerEvent, TorService};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, PhysicalPosition, Rect, State, WebviewWindow, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

const MAX_LOG_LINES: usize = 500;
const MAIN_WINDOW_LABEL: &str = "main";
const PANEL_MARGIN: i32 = 16;
const APP_ICON_BYTES: &[u8] = include_bytes!("../icons/fav1.png");

type TaskHandle = tauri::async_runtime::JoinHandle<()>;

#[derive(Default)]
struct AppState {
    config: Mutex<FoxyTunnelConfig>,
    config_path: StdMutex<Option<PathBuf>>,
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
    message: String,
}

#[derive(Deserialize)]
struct TorCheckResponse {
    #[serde(rename = "IsTor")]
    is_tor: bool,
    #[serde(rename = "IP")]
    ip: Option<String>,
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
            if let Err(error) = persist_config(&app, &state, &config) {
                emit_log(&app, &state, "error", error);
            }
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

async fn check_tor_via_socks(endpoint: &str) -> Result<TorCheckDto, String> {
    let proxy = reqwest::Proxy::all(format!("socks5h://{endpoint}"))
        .map_err(|error| format!("failed to configure SOCKS proxy: {error}"))?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("failed to create Tor check client: {error}"))?;
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

    Ok(tor_check_from_response(response))
}

fn tor_check_from_response(response: TorCheckResponse) -> TorCheckDto {
    if response.is_tor {
        let suffix = response
            .ip
            .as_deref()
            .map_or_else(String::new, |ip| format!(" Exit IP: {ip}."));
        TorCheckDto {
            status: TorCheckStatus::Tor,
            is_tor: true,
            ip: response.ip,
            message: format!("Tor connection verified.{suffix}"),
        }
    } else {
        TorCheckDto {
            status: TorCheckStatus::NotTor,
            is_tor: false,
            ip: response.ip,
            message: "Connection reached Tor Check but was not identified as Tor.".to_string(),
        }
    }
}

impl TorCheckDto {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: TorCheckStatus::Unavailable,
            is_tor: false,
            ip: None,
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

    config.socks_port = options.socks_port;
    config.log_connections = options.log_connections;
    config.bootstrap_timeout_seconds = options.bootstrap_timeout_seconds;

    Ok(())
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

fn endpoint_from_config(config: &FoxyTunnelConfig) -> String {
    format!("{}:{}", config.socks_host, config.socks_port)
}

fn status_from_parts(config: &FoxyTunnelConfig, proxy: &ProxyState) -> StatusDto {
    StatusDto {
        status: proxy.status,
        endpoint: endpoint_from_config(config),
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
            let state = Arc::clone(app.state::<Arc<AppState>>().inner());
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
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            clear_activity_logs,
            get_activity_logs,
            get_status,
            hide_panel_window,
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

    if let Ok(icon) = Image::from_bytes(APP_ICON_BYTES) {
        tray = tray.icon(icon);
    } else if let Some(icon) = app.default_window_icon().cloned() {
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
    let preferred_x = work_right
        .saturating_sub(window_width)
        .saturating_sub(PANEL_MARGIN);
    let preferred_y = work_bottom
        .saturating_sub(window_height)
        .saturating_sub(PANEL_MARGIN);
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
}

#[cfg(test)]
mod tests {
    use super::{
        StartOptions, TorCheckResponse, TorCheckStatus, apply_start_options,
        config_file_path_from_dir, tor_check_from_response, write_config_to_file,
    };
    use foxytunnel_core::FoxyTunnelConfig;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tor_check_json_parses_verified_response() {
        let response: TorCheckResponse =
            serde_json::from_str(r#"{"IsTor":true,"IP":"185.220.101.1"}"#)
                .expect("Tor Check JSON should parse");
        let result = tor_check_from_response(response);

        assert_eq!(result.status, TorCheckStatus::Tor);
        assert!(result.is_tor);
        assert_eq!(result.ip.as_deref(), Some("185.220.101.1"));
    }

    #[test]
    fn config_load_save_roundtrips() {
        let dir = unique_temp_dir();
        let path = config_file_path_from_dir(&dir);
        let config = FoxyTunnelConfig {
            socks_port: 19_051,
            log_connections: true,
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
                log_connections: false,
                bootstrap_timeout_seconds: 5,
            },
        );

        assert!(result.is_err());
        assert_eq!(config.bootstrap_timeout_seconds, 120);
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

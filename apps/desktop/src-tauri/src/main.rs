//! `FoxyTunnel` desktop application entry point.

use foxytunnel_core::{FoxyTunnelConfig, TorService};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;
use tokio::sync::Mutex;

#[derive(Default)]
struct AppState {
    config: Mutex<FoxyTunnelConfig>,
    proxy: Mutex<ProxyState>,
}

#[derive(Default)]
struct ProxyState {
    status: ProxyStatus,
    handle: Option<tauri::async_runtime::JoinHandle<()>>,
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
async fn start_socks(
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
    let result = async {
        let mut service = TorService::create(config.tor_service_config())
            .await
            .map_err(|error| error.to_string())?;
        let timeout = Duration::from_secs(config.bootstrap_timeout_seconds);
        tokio::time::timeout(timeout, service.bootstrap())
            .await
            .map_err(|_| {
                format!(
                    "Tor bootstrap timed out after {} seconds",
                    timeout.as_secs()
                )
            })?
            .map_err(|error| error.to_string())?;

        let socks_config = config.socks_server_config();
        let handle = tauri::async_runtime::spawn(async move {
            if let Err(error) = service.run_socks_proxy(socks_config).await {
                eprintln!("SOCKS proxy stopped: {error}");
            }
        });

        Ok::<_, String>(handle)
    }
    .await;

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
            return Err(error);
        }
    }

    Ok(status_from_parts(&config, &proxy))
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
            get_status,
            start_socks,
            stop_socks
        ])
        .run(tauri::generate_context!())
        .expect("error while running FoxyTunnel desktop app");
}

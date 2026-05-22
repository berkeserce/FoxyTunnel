//! Core state and configuration for `FoxyTunnel`.

mod socks;

use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::TorClientConfigBuilder};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{error::Error, fmt};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tor_rtcompat::PreferredRuntime;

pub use socks::{SocksServer, SocksServerConfig, SocksServerEvent, SocksTarget};

/// Application configuration loaded from TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoxyTunnelConfig {
    /// SOCKS listen host.
    pub socks_host: String,
    /// SOCKS listen port.
    pub socks_port: u16,
    /// Whether accepted SOCKS CONNECT targets should be logged.
    pub log_connections: bool,
    /// Tor bootstrap timeout in seconds.
    pub bootstrap_timeout_seconds: u64,
    /// Arti persistent state directory.
    pub arti_state_dir: PathBuf,
    /// Arti cache directory.
    pub arti_cache_dir: PathBuf,
}

impl Default for FoxyTunnelConfig {
    fn default() -> Self {
        Self {
            socks_host: "127.0.0.1".to_string(),
            socks_port: 19_050,
            log_connections: false,
            bootstrap_timeout_seconds: 120,
            arti_state_dir: PathBuf::from("target/foxytunnel/arti-state"),
            arti_cache_dir: PathBuf::from("target/foxytunnel/arti-cache"),
        }
    }
}

impl FoxyTunnelConfig {
    /// Loads TOML configuration from disk.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the file cannot be read, or a TOML decode error
    /// if the file is malformed.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(ConfigError::Read)?;
        toml::from_str(&contents).map_err(ConfigError::Decode)
    }

    /// Serializes the configuration to pretty TOML.
    ///
    /// # Errors
    ///
    /// Returns a TOML encode error if serialization fails.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::Encode)
    }

    /// Builds a Tor service configuration from app configuration.
    #[must_use]
    pub fn tor_service_config(&self) -> TorServiceConfig {
        TorServiceConfig {
            socks_endpoint: SocksEndpoint {
                host: self.socks_host.clone(),
                port: self.socks_port,
            },
            ..TorServiceConfig::default()
        }
        .with_storage_dirs(self.arti_state_dir.clone(), self.arti_cache_dir.clone())
    }

    /// Builds local SOCKS server configuration from app configuration.
    #[must_use]
    pub fn socks_server_config(&self) -> SocksServerConfig {
        SocksServerConfig {
            log_connections: self.log_connections,
            event_sink: None,
        }
    }
}

/// Configuration loading or serialization error.
#[derive(Debug)]
pub enum ConfigError {
    /// Reading the config file failed.
    Read(std::io::Error),
    /// Parsing TOML failed.
    Decode(toml::de::Error),
    /// Serializing TOML failed.
    Encode(toml::ser::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "failed to read config: {error}"),
            Self::Decode(error) => write!(formatter, "failed to parse config: {error}"),
            Self::Encode(error) => write!(formatter, "failed to serialize config: {error}"),
        }
    }
}

impl Error for ConfigError {}

/// Runtime protection modes exposed by the application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProtectionMode {
    /// `FoxyTunnel` is not routing traffic.
    #[default]
    Off,
    /// `FoxyTunnel` is routing supported system traffic through a tunnel.
    Tunnel,
    /// `FoxyTunnel` launches selected applications with proxy settings.
    App,
}

/// Tor client lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorStatus {
    /// Tor is not running.
    Stopped,
    /// Tor is bootstrapping with a best-effort percentage.
    Bootstrapping(u8),
    /// Tor is ready for streams.
    Ready,
    /// Tor failed with a user-displayable message.
    Failed(String),
}

impl TorStatus {
    /// Returns true when Tor can accept traffic.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Loopback SOCKS endpoint backed by the Tor client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksEndpoint {
    /// Listen host.
    pub host: String,
    /// Listen port.
    pub port: u16,
}

impl Default for SocksEndpoint {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 19_050,
        }
    }
}

impl SocksEndpoint {
    /// Formats the endpoint as `host:port`.
    #[must_use]
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Configuration for the embedded Tor runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TorServiceConfig {
    /// Loopback SOCKS endpoint planned for tunnel and app modes.
    pub socks_endpoint: SocksEndpoint,
    /// Whether stream usage may trigger automatic Tor bootstrap.
    pub bootstrap_on_demand: bool,
    /// Optional storage directories for Arti state and cache.
    pub storage_dirs: Option<TorStorageDirs>,
}

impl TorServiceConfig {
    /// Sets explicit Arti state and cache directories.
    #[must_use]
    pub fn with_storage_dirs(
        mut self,
        state_dir: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
    ) -> Self {
        self.storage_dirs = Some(TorStorageDirs {
            state_dir: state_dir.into(),
            cache_dir: cache_dir.into(),
        });
        self
    }

    /// Returns the matching Arti bootstrap behavior.
    #[must_use]
    pub const fn bootstrap_behavior(&self) -> BootstrapBehavior {
        if self.bootstrap_on_demand {
            BootstrapBehavior::OnDemand
        } else {
            BootstrapBehavior::Manual
        }
    }

    fn arti_config(&self) -> TorServiceResult<TorClientConfig> {
        let Some(storage_dirs) = &self.storage_dirs else {
            return Ok(TorClientConfig::default());
        };

        storage_dirs.prepare()?;

        TorClientConfigBuilder::from_directories(storage_dirs.state_dir(), storage_dirs.cache_dir())
            .build()
            .map_err(|error| TorServiceError::CreateClient(error.to_string()))
    }
}

/// Explicit Arti storage directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorStorageDirs {
    /// Directory for persistent Arti state.
    pub state_dir: PathBuf,
    /// Directory for cached Arti directory information.
    pub cache_dir: PathBuf,
}

impl TorStorageDirs {
    /// Returns the persistent state directory.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Returns the cache directory.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn prepare(&self) -> TorServiceResult<()> {
        fs::create_dir_all(&self.state_dir)
            .and_then(|()| fs::create_dir_all(&self.cache_dir))
            .map_err(|error| TorServiceError::CreateClient(error.to_string()))?;
        set_private_storage_permissions(&self.state_dir)?;
        set_private_storage_permissions(&self.cache_dir)
    }
}

#[cfg(unix)]
fn set_private_storage_permissions(path: &Path) -> TorServiceResult<()> {
    if let Some(parent) = path.parent() {
        set_private_dir_permissions(parent)?;
    }

    set_private_dir_permissions(path)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> TorServiceResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| TorServiceError::CreateClient(error.to_string()))
}

#[cfg(not(unix))]
fn set_private_storage_permissions(_path: &Path) -> TorServiceResult<()> {
    Ok(())
}

/// Errors from the embedded Tor runtime boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorServiceError {
    /// Creating the Arti client failed.
    CreateClient(String),
    /// Tor bootstrap failed.
    Bootstrap(String),
    /// Running the local SOCKS proxy failed.
    SocksProxy(String),
    /// A Tor client operation was requested before a client exists.
    ClientUnavailable,
}

impl fmt::Display for TorServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateClient(message) => {
                write!(formatter, "failed to create Tor client: {message}")
            }
            Self::Bootstrap(message) => write!(formatter, "failed to bootstrap Tor: {message}"),
            Self::SocksProxy(message) => write!(formatter, "SOCKS proxy failed: {message}"),
            Self::ClientUnavailable => formatter.write_str("Tor client is not available"),
        }
    }
}

impl Error for TorServiceError {}

/// Result type for Tor service operations.
pub type TorServiceResult<T> = Result<T, TorServiceError>;

/// Embedded Arti-backed Tor runtime.
#[derive(Clone)]
pub struct TorService {
    config: TorServiceConfig,
    client: Option<TorClient<PreferredRuntime>>,
    status: TorStatus,
}

impl TorService {
    /// Creates an Arti client without connecting to the Tor network yet.
    ///
    /// The default configuration uses manual bootstrap so the app can wait for
    /// explicit user action before making network connections.
    ///
    /// # Errors
    ///
    /// Returns [`TorServiceError::CreateClient`] if Arti cannot create the
    /// underlying client or acquire its local resources.
    pub async fn create(config: TorServiceConfig) -> TorServiceResult<Self> {
        install_crypto_provider();

        let client = TorClient::builder()
            .config(config.arti_config()?)
            .bootstrap_behavior(config.bootstrap_behavior())
            .create_unbootstrapped_async()
            .await
            .map_err(|error| TorServiceError::CreateClient(format_error_chain(&error)))?;

        Ok(Self {
            config,
            client: Some(client),
            status: TorStatus::Stopped,
        })
    }

    /// Returns the immutable service configuration.
    #[must_use]
    pub const fn config(&self) -> &TorServiceConfig {
        &self.config
    }

    /// Returns the current Tor status.
    #[must_use]
    pub const fn status(&self) -> &TorStatus {
        &self.status
    }

    /// Returns the configured SOCKS endpoint.
    #[must_use]
    pub const fn socks_endpoint(&self) -> &SocksEndpoint {
        &self.config.socks_endpoint
    }

    /// Returns a clone of the underlying Arti client.
    ///
    /// Cloning an Arti client is cheap and keeps the same underlying runtime
    /// handles, which is useful when the future SOCKS server needs a client.
    #[must_use]
    pub fn client(&self) -> Option<TorClient<PreferredRuntime>> {
        self.client.clone()
    }

    /// Runs a local SOCKS5 proxy backed by the Arti client.
    ///
    /// # Errors
    ///
    /// Returns [`TorServiceError::ClientUnavailable`] if the service has no
    /// client, or [`TorServiceError::SocksProxy`] if binding or proxying fails.
    pub async fn run_socks_proxy(&self, config: SocksServerConfig) -> TorServiceResult<()> {
        let client = self.client().ok_or(TorServiceError::ClientUnavailable)?;
        let server = SocksServer::new(self.socks_endpoint().clone(), client).with_config(config);

        server
            .run()
            .await
            .map_err(|error| TorServiceError::SocksProxy(error.to_string()))
    }

    /// Bootstraps Tor and marks the service ready if successful.
    ///
    /// # Errors
    ///
    /// Returns [`TorServiceError::ClientUnavailable`] if the service has no
    /// client, or [`TorServiceError::Bootstrap`] if Arti cannot bootstrap.
    pub async fn bootstrap(&mut self) -> TorServiceResult<()> {
        let client = self
            .client
            .as_ref()
            .ok_or(TorServiceError::ClientUnavailable)?;

        self.status = TorStatus::Bootstrapping(0);

        match client.bootstrap().await {
            Ok(()) => {
                self.status = TorStatus::Ready;
                Ok(())
            }
            Err(error) => {
                let message = format_error_chain(&error);
                self.status = TorStatus::Failed(message.clone());
                Err(TorServiceError::Bootstrap(message))
            }
        }
    }
}

fn format_error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();

    while let Some(error) = source {
        message.push_str(": ");
        message.push_str(&error.to_string());
        source = error.source();
    }

    message
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    use super::{
        BootstrapBehavior, FoxyTunnelConfig, ProtectionMode, SocksEndpoint, TorServiceConfig,
        TorServiceError, TorStatus,
    };

    #[test]
    fn protection_mode_defaults_to_off() {
        assert_eq!(ProtectionMode::default(), ProtectionMode::Off);
    }

    #[test]
    fn socks_endpoint_defaults_to_loopback() {
        let endpoint = SocksEndpoint::default();

        assert_eq!(endpoint.authority(), "127.0.0.1:19050");
    }

    #[test]
    fn tor_status_reports_readiness() {
        assert!(TorStatus::Ready.is_ready());
        assert!(!TorStatus::Stopped.is_ready());
    }

    #[test]
    fn tor_service_uses_manual_bootstrap_by_default() {
        let config = TorServiceConfig::default();

        assert_eq!(config.socks_endpoint.authority(), "127.0.0.1:19050");
        assert!(!config.bootstrap_on_demand);
        assert_eq!(config.bootstrap_behavior(), BootstrapBehavior::Manual);
    }

    #[test]
    fn tor_service_can_enable_on_demand_bootstrap() {
        let config = TorServiceConfig {
            bootstrap_on_demand: true,
            ..TorServiceConfig::default()
        };

        assert_eq!(config.bootstrap_behavior(), BootstrapBehavior::OnDemand);
    }

    #[test]
    fn tor_service_error_formats_for_users() {
        let error = TorServiceError::ClientUnavailable;

        assert_eq!(error.to_string(), "Tor client is not available");
    }

    #[test]
    fn socks_proxy_error_formats_for_users() {
        let error = TorServiceError::SocksProxy("bind failed".to_string());

        assert_eq!(error.to_string(), "SOCKS proxy failed: bind failed");
    }

    #[test]
    fn tor_service_config_accepts_storage_dirs() {
        let config =
            TorServiceConfig::default().with_storage_dirs("target/test-state", "target/test-cache");
        let storage_dirs = config.storage_dirs.expect("storage dirs should be set");

        assert_eq!(
            storage_dirs.state_dir(),
            std::path::Path::new("target/test-state")
        );
        assert_eq!(
            storage_dirs.cache_dir(),
            std::path::Path::new("target/test-cache")
        );
    }

    #[test]
    fn app_config_defaults_are_loopback_only() {
        let config = FoxyTunnelConfig::default();

        assert_eq!(config.socks_host, "127.0.0.1");
        assert_eq!(config.socks_port, 19_050);
        assert!(!config.log_connections);
        assert_eq!(config.bootstrap_timeout_seconds, 120);
    }

    #[test]
    fn app_config_builds_runtime_configs() {
        let config = FoxyTunnelConfig {
            socks_port: 19_051,
            log_connections: true,
            ..FoxyTunnelConfig::default()
        };
        let tor_config = config.tor_service_config();
        let socks_config = config.socks_server_config();

        assert_eq!(tor_config.socks_endpoint.authority(), "127.0.0.1:19051");
        assert!(socks_config.log_connections);
    }

    #[test]
    fn app_config_parses_partial_toml() {
        let config: FoxyTunnelConfig = toml::from_str("socks_port = 19052\n")
            .expect("partial config should parse with defaults");

        assert_eq!(config.socks_host, "127.0.0.1");
        assert_eq!(config.socks_port, 19_052);
    }

    #[test]
    fn app_config_serializes_to_toml() {
        let config = FoxyTunnelConfig::default();
        let toml = config.to_toml_string().expect("config should serialize");

        assert!(toml.contains("socks_host"));
        assert!(toml.contains("bootstrap_timeout_seconds"));
    }
}

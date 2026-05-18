//! Core state and configuration for `FoxyTunnel`.

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

#[cfg(test)]
mod tests {
    use super::{ProtectionMode, SocksEndpoint, TorStatus};

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
}

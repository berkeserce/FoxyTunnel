//! Tunnel planning and platform backend boundaries for `FoxyTunnel`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

use foxytunnel_core::SocksEndpoint;
use std::{error::Error, fmt};

#[cfg(target_os = "linux")]
pub use linux::LinuxTunnelBackend as PlatformTunnelBackend;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub use unsupported::UnsupportedTunnelBackend as PlatformTunnelBackend;
#[cfg(target_os = "windows")]
pub use windows::WindowsTunnelBackend as PlatformTunnelBackend;

/// User-visible tunnel lifecycle state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TunnelState {
    /// No tunnel is active.
    #[default]
    Down,
    /// Tunnel setup is in progress.
    Starting,
    /// Supported traffic is routed through the tunnel.
    Up,
    /// Tunnel teardown is in progress.
    Stopping,
    /// Tunnel setup or runtime failed.
    Failed(String),
}

/// Errors returned by platform tunnel backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelError {
    /// The current platform has no tunnel implementation yet.
    UnsupportedPlatform {
        /// Static platform identifier used in user-facing diagnostics.
        platform: &'static str,
    },
    /// A platform backend failed with a user-displayable message.
    Backend(String),
}

impl fmt::Display for TunnelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform { platform } => {
                write!(formatter, "tunnel mode is not implemented for {platform}")
            }
            Self::Backend(message) => formatter.write_str(message),
        }
    }
}

impl Error for TunnelError {}

/// Result type for tunnel backend operations.
pub type TunnelResult<T> = Result<T, TunnelError>;

/// Platform-specific tunnel lifecycle boundary.
///
/// `FoxyTunnel` keeps Tor and SOCKS behavior in `foxytunnel-core`; this trait is
/// the boundary for OS-specific packet capture, routing, DNS, and UDP policy.
pub trait TunnelBackend {
    /// Starts the tunnel with the provided plan.
    ///
    /// # Errors
    ///
    /// Returns a platform-specific error if setup fails or the backend is not
    /// implemented on the current platform yet.
    fn start(&mut self, plan: TunnelPlan) -> TunnelResult<()>;

    /// Stops the tunnel and cleans up platform routing state.
    ///
    /// # Errors
    ///
    /// Returns a platform-specific error if teardown fails.
    fn stop(&mut self) -> TunnelResult<()>;

    /// Returns the current tunnel state.
    #[must_use]
    fn state(&self) -> TunnelState;

    /// Returns true when this backend can start a real system tunnel.
    #[must_use]
    fn is_supported(&self) -> bool;
}

/// Static tunnel setup choices before platform-specific code is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelPlan {
    /// Platform adapter or interface display name.
    pub adapter_name: String,
    /// Local SOCKS endpoint that receives tunnel traffic.
    pub socks_endpoint: SocksEndpoint,
    /// Whether unsupported UDP traffic should be blocked.
    pub block_udp: bool,
}

impl Default for TunnelPlan {
    fn default() -> Self {
        Self {
            adapter_name: "FoxyTunnel".to_string(),
            socks_endpoint: SocksEndpoint::default(),
            block_udp: true,
        }
    }
}

impl TunnelPlan {
    /// Returns true when the plan avoids silently allowing unsupported UDP.
    #[must_use]
    pub const fn has_udp_leak_protection(&self) -> bool {
        self.block_udp
    }
}

#[cfg(test)]
mod tests {
    use super::{PlatformTunnelBackend, TunnelBackend, TunnelError, TunnelPlan, TunnelState};

    #[test]
    fn tunnel_state_defaults_to_down() {
        assert_eq!(TunnelState::default(), TunnelState::Down);
    }

    #[test]
    fn default_plan_blocks_udp() {
        let plan = TunnelPlan::default();

        assert!(plan.has_udp_leak_protection());
        assert_eq!(plan.adapter_name, "FoxyTunnel");
        assert_eq!(plan.socks_endpoint.authority(), "127.0.0.1:19050");
    }

    #[test]
    fn unsupported_platform_error_formats_for_users() {
        let error = TunnelError::UnsupportedPlatform { platform: "linux" };

        assert_eq!(
            error.to_string(),
            "tunnel mode is not implemented for linux"
        );
    }

    #[test]
    fn platform_backend_starts_as_down() {
        let backend = PlatformTunnelBackend::default();

        assert_eq!(backend.state(), TunnelState::Down);
    }
}

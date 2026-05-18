//! Tunnel planning for `FoxyTunnel`.

use foxytunnel_core::SocksEndpoint;

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

/// Static tunnel setup choices before platform-specific code is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelPlan {
    /// Windows adapter display name.
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
    use super::{TunnelPlan, TunnelState};

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
}

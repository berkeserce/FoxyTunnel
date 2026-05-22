//! Linux tunnel backend placeholder.

use crate::{TunnelBackend, TunnelError, TunnelPlan, TunnelResult, TunnelState};

/// Linux system tunnel backend.
///
/// The real implementation will manage a TUN device, routing policy, DNS
/// handling, and unsupported UDP/ICMP behavior. For now it is an explicit
/// placeholder so Linux builds have a stable platform boundary.
#[derive(Debug, Clone, Default)]
pub struct LinuxTunnelBackend {
    state: TunnelState,
}

impl TunnelBackend for LinuxTunnelBackend {
    fn start(&mut self, _plan: TunnelPlan) -> TunnelResult<()> {
        self.state = TunnelState::Failed("Linux tunnel backend is not implemented yet".to_string());

        Err(TunnelError::UnsupportedPlatform { platform: "linux" })
    }

    fn stop(&mut self) -> TunnelResult<()> {
        self.state = TunnelState::Down;

        Ok(())
    }

    fn state(&self) -> TunnelState {
        self.state.clone()
    }

    fn is_supported(&self) -> bool {
        false
    }
}

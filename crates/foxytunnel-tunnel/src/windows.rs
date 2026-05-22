//! Windows tunnel backend placeholder.

use crate::{TunnelBackend, TunnelError, TunnelPlan, TunnelResult, TunnelState};

/// Windows system tunnel backend.
///
/// The real implementation will manage Wintun or equivalent adapter setup,
/// tun2proxy integration, route updates, DNS policy, and unsupported UDP/ICMP
/// behavior. For now it is an explicit placeholder behind the Windows target.
#[derive(Debug, Clone, Default)]
pub struct WindowsTunnelBackend {
    state: TunnelState,
}

impl TunnelBackend for WindowsTunnelBackend {
    fn start(&mut self, _plan: TunnelPlan) -> TunnelResult<()> {
        self.state =
            TunnelState::Failed("Windows tunnel backend is not implemented yet".to_string());

        Err(TunnelError::UnsupportedPlatform {
            platform: "windows",
        })
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

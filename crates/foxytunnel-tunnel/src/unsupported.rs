//! Fallback tunnel backend for unsupported platforms.

use crate::{TunnelBackend, TunnelError, TunnelPlan, TunnelResult, TunnelState};

/// Tunnel backend used when no platform-specific implementation is available.
#[derive(Debug, Clone, Default)]
pub struct UnsupportedTunnelBackend {
    state: TunnelState,
}

impl TunnelBackend for UnsupportedTunnelBackend {
    fn start(&mut self, _plan: TunnelPlan) -> TunnelResult<()> {
        self.state = TunnelState::Failed("Tunnel backend is not implemented yet".to_string());

        Err(TunnelError::UnsupportedPlatform {
            platform: "this platform",
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

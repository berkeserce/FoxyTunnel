//! Temporary `FoxyTunnel` entry point before the Tauri tray shell is added.

use foxytunnel_core::{ProtectionMode, SocksEndpoint};
use foxytunnel_tunnel::TunnelPlan;

fn main() {
    let mode = ProtectionMode::default();
    let endpoint = SocksEndpoint::default();
    let tunnel_plan = TunnelPlan::default();

    println!("FoxyTunnel status: {mode:?}");
    println!("SOCKS endpoint: {}", endpoint.authority());
    println!("Tunnel adapter: {}", tunnel_plan.adapter_name);
}

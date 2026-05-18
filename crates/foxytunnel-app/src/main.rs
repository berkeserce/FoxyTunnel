//! Temporary `FoxyTunnel` entry point before the Tauri tray shell is added.

use foxytunnel_core::{ProtectionMode, TorService, TorServiceConfig};
use foxytunnel_tunnel::TunnelPlan;

fn main() {
    if let Err(error) = run() {
        eprintln!("FoxyTunnel failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    install_crypto_provider();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mode = ProtectionMode::default();
    let tunnel_plan = TunnelPlan::default();
    let tor_config = TorServiceConfig::default().with_storage_dirs(
        "target/foxytunnel/arti-state",
        "target/foxytunnel/arti-cache",
    );
    let tor_service = runtime.block_on(TorService::create(tor_config))?;

    println!("FoxyTunnel status: {mode:?}");
    println!("Tor status: {:?}", tor_service.status());
    println!(
        "SOCKS endpoint: {}",
        tor_service.socks_endpoint().authority()
    );
    println!("Tunnel adapter: {}", tunnel_plan.adapter_name);

    Ok(())
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

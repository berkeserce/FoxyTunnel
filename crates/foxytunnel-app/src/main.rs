//! Temporary `FoxyTunnel` entry point before the Tauri tray shell is added.

use foxytunnel_core::{ProtectionMode, TorService, TorServiceConfig};
use foxytunnel_tunnel::TunnelPlan;
use std::{env, io, time::Duration};

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    if let Err(error) = run() {
        eprintln!("FoxyTunnel failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    install_crypto_provider();

    let command = AppCommand::parse(env::args().skip(1))?;
    if command == AppCommand::Help {
        print_help();
        return Ok(());
    }

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

    match command {
        AppCommand::Status => print_status(mode, &tor_service, &tunnel_plan),
        AppCommand::Bootstrap => {
            let mut tor_service = tor_service;

            println!("Bootstrapping Tor...");
            bootstrap_tor(&runtime, &mut tor_service)?;

            print_status(mode, &tor_service, &tunnel_plan);
        }
        AppCommand::Socks => {
            let mut tor_service = tor_service;

            println!("Bootstrapping Tor...");
            bootstrap_tor(&runtime, &mut tor_service)?;

            println!(
                "SOCKS5 listening on {}",
                tor_service.socks_endpoint().authority()
            );
            runtime.block_on(tor_service.run_socks_proxy())?;
        }
        AppCommand::Help => unreachable!("help exits before runtime startup"),
    }

    Ok(())
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn bootstrap_tor(
    runtime: &tokio::runtime::Runtime,
    tor_service: &mut TorService,
) -> Result<(), Box<dyn std::error::Error>> {
    match runtime
        .block_on(async { tokio::time::timeout(BOOTSTRAP_TIMEOUT, tor_service.bootstrap()).await })
    {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "Tor bootstrap timed out after {} seconds",
                BOOTSTRAP_TIMEOUT.as_secs()
            ),
        )
        .into()),
    }
}

fn print_status(mode: ProtectionMode, tor_service: &TorService, tunnel_plan: &TunnelPlan) {
    println!("FoxyTunnel status: {mode:?}");
    println!("Tor status: {:?}", tor_service.status());
    println!(
        "SOCKS endpoint: {}",
        tor_service.socks_endpoint().authority()
    );
    println!("Tunnel adapter: {}", tunnel_plan.adapter_name);
}

fn print_help() {
    println!("FoxyTunnel");
    println!();
    println!("Usage:");
    println!("  foxytunnel-app");
    println!("  foxytunnel-app --bootstrap");
    println!("  foxytunnel-app --socks");
    println!("  foxytunnel-app --help");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppCommand {
    Status,
    Bootstrap,
    Socks,
    Help,
}

impl AppCommand {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, io::Error> {
        let mut command = Self::Status;

        for arg in args {
            command = match arg.as_str() {
                "--bootstrap" | "bootstrap" => Self::Bootstrap,
                "--socks" | "socks" => Self::Socks,
                "--help" | "-h" | "help" => Self::Help,
                unknown => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown argument: {unknown}"),
                    ));
                }
            };
        }

        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::AppCommand;

    #[test]
    fn command_defaults_to_status() {
        assert_eq!(AppCommand::parse([]).expect("command"), AppCommand::Status);
    }

    #[test]
    fn command_accepts_bootstrap_flag() {
        assert_eq!(
            AppCommand::parse(["--bootstrap".to_string()]).expect("command"),
            AppCommand::Bootstrap
        );
    }

    #[test]
    fn command_accepts_socks_flag() {
        assert_eq!(
            AppCommand::parse(["--socks".to_string()]).expect("command"),
            AppCommand::Socks
        );
    }

    #[test]
    fn command_rejects_unknown_arguments() {
        assert!(AppCommand::parse(["--wat".to_string()]).is_err());
    }
}

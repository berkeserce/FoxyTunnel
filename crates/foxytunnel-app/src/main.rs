//! Temporary `FoxyTunnel` entry point before the Tauri tray shell is added.

use foxytunnel_core::{ProtectionMode, SocksServerConfig, TorService, TorServiceConfig};
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

    let options = AppOptions::parse(env::args().skip(1))?;
    if options.command == AppCommand::Help {
        print_help();
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mode = ProtectionMode::default();
    let tunnel_plan = TunnelPlan::default();
    let tor_config = TorServiceConfig {
        socks_endpoint: foxytunnel_core::SocksEndpoint {
            host: "127.0.0.1".to_string(),
            port: options.socks_port,
        },
        ..TorServiceConfig::default()
    }
    .with_storage_dirs(
        "target/foxytunnel/arti-state",
        "target/foxytunnel/arti-cache",
    );
    let tor_service = runtime.block_on(TorService::create(tor_config))?;

    match options.command {
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
            runtime.block_on(tor_service.run_socks_proxy(SocksServerConfig {
                log_connections: options.log_connections,
            }))?;
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
    println!("  foxytunnel-app [--port <port>]");
    println!("  foxytunnel-app --bootstrap");
    println!("  foxytunnel-app --socks [--port <port>] [--log]");
    println!("  foxytunnel-app --help");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppOptions {
    command: AppCommand,
    socks_port: u16,
    log_connections: bool,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            command: AppCommand::Status,
            socks_port: 19_050,
            log_connections: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppCommand {
    Status,
    Bootstrap,
    Socks,
    Help,
}

impl AppOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, io::Error> {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bootstrap" | "bootstrap" => options.command = AppCommand::Bootstrap,
                "--socks" | "socks" => options.command = AppCommand::Socks,
                "--status" | "status" => options.command = AppCommand::Status,
                "--help" | "-h" | "help" => options.command = AppCommand::Help,
                "--log" => options.log_connections = true,
                "--port" | "-p" => {
                    let Some(port) = args.next() else {
                        return Err(invalid_input("--port requires a value"));
                    };
                    options.socks_port = parse_port(&port)?;
                }
                _ if arg.starts_with("--port=") => {
                    let port = arg.trim_start_matches("--port=");
                    options.socks_port = parse_port(port)?;
                }
                unknown => {
                    return Err(invalid_input(format!("unknown argument: {unknown}")));
                }
            }
        }

        Ok(options)
    }
}

fn parse_port(value: &str) -> Result<u16, io::Error> {
    let port = value
        .parse::<u16>()
        .map_err(|_| invalid_input(format!("invalid port: {value}")))?;

    if port == 0 {
        Err(invalid_input("port must be between 1 and 65535"))
    } else {
        Ok(port)
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::{AppCommand, AppOptions};

    #[test]
    fn command_defaults_to_status() {
        let options = AppOptions::parse([]).expect("options");

        assert_eq!(options.command, AppCommand::Status);
        assert_eq!(options.socks_port, 19_050);
        assert!(!options.log_connections);
    }

    #[test]
    fn command_accepts_bootstrap_flag() {
        assert_eq!(
            AppOptions::parse(["--bootstrap".to_string()])
                .expect("options")
                .command,
            AppCommand::Bootstrap
        );
    }

    #[test]
    fn command_accepts_socks_flag() {
        assert_eq!(
            AppOptions::parse(["--socks".to_string()])
                .expect("options")
                .command,
            AppCommand::Socks
        );
    }

    #[test]
    fn command_accepts_status_flag() {
        assert_eq!(
            AppOptions::parse(["--status".to_string()])
                .expect("options")
                .command,
            AppCommand::Status
        );
    }

    #[test]
    fn options_accept_port_value() {
        let options = AppOptions::parse([
            "--socks".to_string(),
            "--port".to_string(),
            "19051".to_string(),
        ])
        .expect("options");

        assert_eq!(options.command, AppCommand::Socks);
        assert_eq!(options.socks_port, 19_051);
    }

    #[test]
    fn options_accept_port_equals_value() {
        let options = AppOptions::parse(["--port=19052".to_string()]).expect("options");

        assert_eq!(options.socks_port, 19_052);
    }

    #[test]
    fn options_accept_connection_logging() {
        let options = AppOptions::parse(["--log".to_string()]).expect("options");

        assert!(options.log_connections);
    }

    #[test]
    fn options_reject_zero_port() {
        assert!(AppOptions::parse(["--port".to_string(), "0".to_string()]).is_err());
    }

    #[test]
    fn command_rejects_unknown_arguments() {
        assert!(AppOptions::parse(["--wat".to_string()]).is_err());
    }
}

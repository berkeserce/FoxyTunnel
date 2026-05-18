//! Temporary `FoxyTunnel` entry point before the Tauri tray shell is added.

use foxytunnel_core::{FoxyTunnelConfig, ProtectionMode, TorService};
use foxytunnel_tunnel::TunnelPlan;
use std::{env, io, path::PathBuf, time::Duration};

fn main() {
    if let Err(error) = run() {
        eprintln!("FoxyTunnel failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = AppOptions::parse(env::args().skip(1))?;
    if options.command == AppCommand::Help {
        print_help();
        return Ok(());
    }

    let config = load_config(&options)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mode = ProtectionMode::default();
    let tunnel_plan = TunnelPlan::default();
    let tor_service = runtime.block_on(TorService::create(config.tor_service_config()))?;

    match options.command {
        AppCommand::Status => print_status(mode, &tor_service, &tunnel_plan),
        AppCommand::Bootstrap => {
            let mut tor_service = tor_service;

            println!("Bootstrapping Tor...");
            bootstrap_tor(&runtime, &mut tor_service, config.bootstrap_timeout())?;

            print_status(mode, &tor_service, &tunnel_plan);
        }
        AppCommand::Socks => {
            let mut tor_service = tor_service;

            println!("Bootstrapping Tor...");
            bootstrap_tor(&runtime, &mut tor_service, config.bootstrap_timeout())?;

            println!(
                "SOCKS5 listening on {}",
                tor_service.socks_endpoint().authority()
            );
            runtime.block_on(tor_service.run_socks_proxy(config.socks_server_config()))?;
        }
        AppCommand::Help => unreachable!("help exits before runtime startup"),
    }

    Ok(())
}

fn bootstrap_tor(
    runtime: &tokio::runtime::Runtime,
    tor_service: &mut TorService,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    match runtime.block_on(async { tokio::time::timeout(timeout, tor_service.bootstrap()).await }) {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "Tor bootstrap timed out after {} seconds",
                timeout.as_secs()
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
    println!("  foxytunnel-app [--config <path>] [--port <port>]");
    println!("  foxytunnel-app --bootstrap [--config <path>]");
    println!("  foxytunnel-app --socks [--config <path>] [--port <port>] [--log]");
    println!("  foxytunnel-app --help");
}

fn load_config(options: &AppOptions) -> Result<FoxyTunnelConfig, Box<dyn std::error::Error>> {
    let mut config = match &options.config_path {
        Some(path) => FoxyTunnelConfig::load(path)?,
        None => FoxyTunnelConfig::default(),
    };

    options.apply_overrides(&mut config);

    Ok(config)
}

trait BootstrapTimeout {
    fn bootstrap_timeout(&self) -> Duration;
}

impl BootstrapTimeout for FoxyTunnelConfig {
    fn bootstrap_timeout(&self) -> Duration {
        Duration::from_secs(self.bootstrap_timeout_seconds)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppOptions {
    command: AppCommand,
    socks_port: Option<u16>,
    log_connections: Option<bool>,
    bootstrap_timeout_seconds: Option<u64>,
    config_path: Option<PathBuf>,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            command: AppCommand::Status,
            socks_port: None,
            log_connections: None,
            bootstrap_timeout_seconds: None,
            config_path: None,
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
                "--log" => options.log_connections = Some(true),
                "--port" | "-p" => {
                    let Some(port) = args.next() else {
                        return Err(invalid_input("--port requires a value"));
                    };
                    options.socks_port = Some(parse_port(&port)?);
                }
                _ if arg.starts_with("--port=") => {
                    let port = arg.trim_start_matches("--port=");
                    options.socks_port = Some(parse_port(port)?);
                }
                "--timeout" => {
                    let Some(timeout) = args.next() else {
                        return Err(invalid_input("--timeout requires a value"));
                    };
                    options.bootstrap_timeout_seconds = Some(parse_positive_u64(
                        &timeout,
                        "timeout must be at least 1 second",
                    )?);
                }
                _ if arg.starts_with("--timeout=") => {
                    let timeout = arg.trim_start_matches("--timeout=");
                    options.bootstrap_timeout_seconds = Some(parse_positive_u64(
                        timeout,
                        "timeout must be at least 1 second",
                    )?);
                }
                "--config" | "-c" => {
                    let Some(path) = args.next() else {
                        return Err(invalid_input("--config requires a value"));
                    };
                    options.config_path = Some(PathBuf::from(path));
                }
                _ if arg.starts_with("--config=") => {
                    let path = arg.trim_start_matches("--config=");
                    options.config_path = Some(PathBuf::from(path));
                }
                unknown => {
                    return Err(invalid_input(format!("unknown argument: {unknown}")));
                }
            }
        }

        Ok(options)
    }

    fn apply_overrides(&self, config: &mut FoxyTunnelConfig) {
        if let Some(port) = self.socks_port {
            config.socks_port = port;
        }

        if let Some(log_connections) = self.log_connections {
            config.log_connections = log_connections;
        }

        if let Some(timeout) = self.bootstrap_timeout_seconds {
            config.bootstrap_timeout_seconds = timeout;
        }
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

fn parse_positive_u64(value: &str, zero_message: &str) -> Result<u64, io::Error> {
    let number = value
        .parse::<u64>()
        .map_err(|_| invalid_input(format!("invalid number: {value}")))?;

    if number == 0 {
        Err(invalid_input(zero_message))
    } else {
        Ok(number)
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::{AppCommand, AppOptions};
    use foxytunnel_core::FoxyTunnelConfig;
    use std::path::PathBuf;

    #[test]
    fn command_defaults_to_status() {
        let options = AppOptions::parse([]).expect("options");

        assert_eq!(options.command, AppCommand::Status);
        assert_eq!(options.socks_port, None);
        assert_eq!(options.log_connections, None);
        assert_eq!(options.config_path, None);
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
        assert_eq!(options.socks_port, Some(19_051));
    }

    #[test]
    fn options_accept_port_equals_value() {
        let options = AppOptions::parse(["--port=19052".to_string()]).expect("options");

        assert_eq!(options.socks_port, Some(19_052));
    }

    #[test]
    fn options_accept_connection_logging() {
        let options = AppOptions::parse(["--log".to_string()]).expect("options");

        assert_eq!(options.log_connections, Some(true));
    }

    #[test]
    fn options_accept_timeout() {
        let options =
            AppOptions::parse(["--timeout".to_string(), "30".to_string()]).expect("options");

        assert_eq!(options.bootstrap_timeout_seconds, Some(30));
    }

    #[test]
    fn options_accept_config_path() {
        let options = AppOptions::parse(["--config".to_string(), "foxytunnel.toml".to_string()])
            .expect("options");

        assert_eq!(options.config_path, Some(PathBuf::from("foxytunnel.toml")));
    }

    #[test]
    fn options_reject_zero_port() {
        assert!(AppOptions::parse(["--port".to_string(), "0".to_string()]).is_err());
    }

    #[test]
    fn command_rejects_unknown_arguments() {
        assert!(AppOptions::parse(["--wat".to_string()]).is_err());
    }

    #[test]
    fn options_override_config_values() {
        let options = AppOptions::parse([
            "--port".to_string(),
            "19053".to_string(),
            "--log".to_string(),
            "--timeout=45".to_string(),
        ])
        .expect("options");
        let mut config = FoxyTunnelConfig::default();

        options.apply_overrides(&mut config);

        assert_eq!(config.socks_port, 19_053);
        assert!(config.log_connections);
        assert_eq!(config.bootstrap_timeout_seconds, 45);
    }
}

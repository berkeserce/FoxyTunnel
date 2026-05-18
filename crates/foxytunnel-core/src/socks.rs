//! Minimal SOCKS5 server backed by Arti.

use crate::SocksEndpoint;
use arti_client::TorClient;
use std::{
    error::Error,
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tor_rtcompat::PreferredRuntime;

const SOCKS_VERSION: u8 = 0x05;
const METHOD_NO_AUTH: u8 = 0x00;
const METHOD_NO_ACCEPTABLE: u8 = 0xff;
const COMMAND_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;
const REPLY_SUCCEEDED: u8 = 0x00;
const REPLY_GENERAL_FAILURE: u8 = 0x01;

/// A parsed SOCKS5 CONNECT target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksTarget {
    /// Domain target with port.
    Domain {
        /// Target host name.
        host: String,
        /// Target TCP port.
        port: u16,
    },
    /// IP target with port.
    Ip {
        /// Target IP address.
        address: IpAddr,
        /// Target TCP port.
        port: u16,
    },
}

impl SocksTarget {
    fn port(&self) -> u16 {
        match self {
            Self::Domain { port, .. } | Self::Ip { port, .. } => *port,
        }
    }
}

impl fmt::Display for SocksTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain { host, port } => write!(formatter, "{host}:{port}"),
            Self::Ip { address, port } => write!(formatter, "{address}:{port}"),
        }
    }
}

/// Errors from the SOCKS5 server.
#[derive(Debug)]
pub enum SocksServerError {
    /// Socket IO failed.
    Io(io::Error),
    /// Client sent an unsupported or malformed SOCKS request.
    Protocol(String),
    /// Arti failed to connect to the requested target.
    Tor(String),
}

impl fmt::Display for SocksServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "socket error: {error}"),
            Self::Protocol(message) => write!(formatter, "SOCKS protocol error: {message}"),
            Self::Tor(message) => write!(formatter, "Tor stream error: {message}"),
        }
    }
}

impl Error for SocksServerError {}

impl From<io::Error> for SocksServerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Result type for SOCKS server operations.
pub type SocksServerResult<T> = Result<T, SocksServerError>;

/// Callback type for SOCKS server events.
pub type SocksEventSink = Arc<dyn Fn(SocksServerEvent) + Send + Sync>;

/// Runtime events emitted by the SOCKS server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksServerEvent {
    /// Listener is bound and ready for local clients.
    Listening(String),
    /// A SOCKS CONNECT target was accepted.
    Connect(String),
    /// A per-connection SOCKS error happened.
    ConnectionFailed(String),
}

/// Local SOCKS5 server backed by an Arti client.
pub struct SocksServer {
    endpoint: SocksEndpoint,
    client: TorClient<PreferredRuntime>,
    config: SocksServerConfig,
}

/// Runtime configuration for the local SOCKS5 server.
#[derive(Clone, Default)]
pub struct SocksServerConfig {
    /// Whether accepted CONNECT targets should be printed to stderr.
    pub log_connections: bool,
    /// Optional event sink for GUI or service-layer logs.
    pub event_sink: Option<SocksEventSink>,
}

impl fmt::Debug for SocksServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocksServerConfig")
            .field("log_connections", &self.log_connections)
            .field("event_sink", &self.event_sink.is_some())
            .finish()
    }
}

impl SocksServer {
    /// Creates a local SOCKS5 server.
    #[must_use]
    pub fn new(endpoint: SocksEndpoint, client: TorClient<PreferredRuntime>) -> Self {
        Self {
            endpoint,
            client,
            config: SocksServerConfig::default(),
        }
    }

    /// Sets runtime server configuration.
    #[must_use]
    pub fn with_config(mut self, config: SocksServerConfig) -> Self {
        self.config = config;
        self
    }

    /// Runs the SOCKS5 server until the task is cancelled or the listener fails.
    ///
    /// # Errors
    ///
    /// Returns [`SocksServerError::Io`] if binding or accepting local sockets
    /// fails.
    pub async fn run(self) -> SocksServerResult<()> {
        let authority = self.endpoint.authority();
        let listener = TcpListener::bind(&authority).await?;
        self.config
            .emit(SocksServerEvent::Listening(authority.clone()));

        loop {
            let (stream, _) = listener.accept().await?;
            let client = self.client.clone();
            let config = self.config.clone();

            tokio::spawn(async move {
                if let Err(error) = handle_connection(stream, client, config).await {
                    eprintln!("SOCKS connection failed: {error}");
                }
            });
        }
    }
}

impl SocksServerConfig {
    fn emit(&self, event: SocksServerEvent) {
        if let Some(sink) = &self.event_sink {
            sink(event);
        }
    }
}

async fn handle_connection(
    mut inbound: TcpStream,
    client: TorClient<PreferredRuntime>,
    config: SocksServerConfig,
) -> SocksServerResult<()> {
    negotiate_no_auth(&mut inbound).await?;
    let target = read_connect_target(&mut inbound).await?;
    if config.log_connections {
        eprintln!("SOCKS CONNECT {target}");
        config.emit(SocksServerEvent::Connect(target.to_string()));
    }

    let mut outbound = match connect_tor(&client, &target).await {
        Ok(stream) => stream,
        Err(error) => {
            write_reply(&mut inbound, REPLY_GENERAL_FAILURE).await?;
            config.emit(SocksServerEvent::ConnectionFailed(format!(
                "failed to connect to {target}: {error}"
            )));
            return Err(SocksServerError::Tor(format!(
                "failed to connect to {target}: {error}"
            )));
        }
    };

    write_reply(&mut inbound, REPLY_SUCCEEDED).await?;
    if let Err(error) = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await
        && !is_expected_disconnect(&error)
    {
        return Err(error.into());
    }

    Ok(())
}

fn is_expected_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
    )
}

async fn connect_tor(
    client: &TorClient<PreferredRuntime>,
    target: &SocksTarget,
) -> arti_client::Result<arti_client::DataStream> {
    match target {
        SocksTarget::Domain { host, port } => client.connect((host.as_str(), *port)).await,
        SocksTarget::Ip { address, port } => client.connect((address.to_string(), *port)).await,
    }
}

async fn negotiate_no_auth(stream: &mut TcpStream) -> SocksServerResult<()> {
    let version = stream.read_u8().await?;
    if version != SOCKS_VERSION {
        return Err(SocksServerError::Protocol(format!(
            "unsupported version {version}"
        )));
    }

    let method_count = stream.read_u8().await?;
    let mut methods = vec![0; usize::from(method_count)];
    stream.read_exact(&mut methods).await?;

    if methods.contains(&METHOD_NO_AUTH) {
        stream.write_all(&[SOCKS_VERSION, METHOD_NO_AUTH]).await?;
        Ok(())
    } else {
        stream
            .write_all(&[SOCKS_VERSION, METHOD_NO_ACCEPTABLE])
            .await?;
        Err(SocksServerError::Protocol(
            "client did not offer no-auth SOCKS5".to_string(),
        ))
    }
}

async fn read_connect_target(stream: &mut TcpStream) -> SocksServerResult<SocksTarget> {
    let version = stream.read_u8().await?;
    if version != SOCKS_VERSION {
        return Err(SocksServerError::Protocol(format!(
            "unsupported request version {version}"
        )));
    }

    let command = stream.read_u8().await?;
    if command != COMMAND_CONNECT {
        write_reply(stream, REPLY_GENERAL_FAILURE).await?;
        return Err(SocksServerError::Protocol(format!(
            "unsupported command {command}"
        )));
    }

    let reserved = stream.read_u8().await?;
    if reserved != 0 {
        write_reply(stream, REPLY_GENERAL_FAILURE).await?;
        return Err(SocksServerError::Protocol(
            "reserved byte must be zero".to_string(),
        ));
    }

    let atyp = stream.read_u8().await?;
    let target = match atyp {
        ATYP_IPV4 => {
            let mut octets = [0; 4];
            stream.read_exact(&mut octets).await?;
            let port = stream.read_u16().await?;

            SocksTarget::Ip {
                address: IpAddr::V4(Ipv4Addr::from(octets)),
                port,
            }
        }
        ATYP_IPV6 => {
            let mut octets = [0; 16];
            stream.read_exact(&mut octets).await?;
            let port = stream.read_u16().await?;

            SocksTarget::Ip {
                address: IpAddr::V6(Ipv6Addr::from(octets)),
                port,
            }
        }
        ATYP_DOMAIN => {
            let len = usize::from(stream.read_u8().await?);
            let mut bytes = vec![0; len];
            stream.read_exact(&mut bytes).await?;
            let port = stream.read_u16().await?;
            let host = String::from_utf8(bytes).map_err(|error| {
                SocksServerError::Protocol(format!("invalid domain name: {error}"))
            })?;

            SocksTarget::Domain { host, port }
        }
        unknown => {
            write_reply(stream, REPLY_GENERAL_FAILURE).await?;
            return Err(SocksServerError::Protocol(format!(
                "unsupported address type {unknown}"
            )));
        }
    };

    if target.port() == 0 {
        write_reply(stream, REPLY_GENERAL_FAILURE).await?;
        return Err(SocksServerError::Protocol(
            "port must not be zero".to_string(),
        ));
    }

    Ok(target)
}

async fn write_reply(stream: &mut TcpStream, reply: u8) -> io::Result<()> {
    stream
        .write_all(&[
            SOCKS_VERSION,
            reply,
            0x00,
            ATYP_IPV4,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        ])
        .await
}

#[cfg(test)]
mod tests {
    use super::{SocksServerConfig, SocksServerEvent, SocksTarget};
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};

    #[test]
    fn socks_server_config_disables_connection_logs_by_default() {
        assert!(!SocksServerConfig::default().log_connections);
    }

    #[test]
    fn domain_target_formats_for_logs() {
        let target = SocksTarget::Domain {
            host: "example.com".to_string(),
            port: 443,
        };

        assert_eq!(target.to_string(), "example.com:443");
    }

    #[test]
    fn ip_target_formats_for_logs() {
        let target = SocksTarget::Ip {
            address: IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            port: 80,
        };

        assert_eq!(target.to_string(), "93.184.216.34:80");
    }

    #[test]
    fn expected_disconnects_are_not_reported_as_proxy_failures() {
        let error = std::io::Error::new(std::io::ErrorKind::NotConnected, "closed");

        assert!(super::is_expected_disconnect(&error));
    }

    #[test]
    fn socks_server_config_emits_events_to_sink() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = Arc::clone(&events);
        let config = SocksServerConfig {
            event_sink: Some(Arc::new(move |event| {
                captured_events.lock().expect("events lock").push(event);
            })),
            ..SocksServerConfig::default()
        };

        config.emit(SocksServerEvent::Connect("example.com:443".to_string()));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            &[SocksServerEvent::Connect("example.com:443".to_string())]
        );
    }
}

use crate::system_proxy::ProxyEndpoint;
use std::{fmt, io, str, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MAX_HEADER_BYTES: usize = 64 * 1024;

type HttpProxyEventSink = Arc<dyn Fn(HttpProxyEvent) + Send + Sync>;

#[derive(Clone, Default)]
pub(crate) struct HttpProxyConfig {
    pub(crate) event_sink: Option<HttpProxyEventSink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpProxyEvent {
    Listening(String),
    ConnectionFailed(String),
}

#[derive(Debug)]
pub(crate) enum HttpProxyError {
    Io(io::Error),
    Protocol(String),
    Socks(String),
}

impl fmt::Display for HttpProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "socket error: {error}"),
            Self::Protocol(message) => write!(formatter, "HTTP proxy protocol error: {message}"),
            Self::Socks(message) => write!(formatter, "SOCKS bridge error: {message}"),
        }
    }
}

impl From<io::Error> for HttpProxyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) type HttpProxyResult<T> = Result<T, HttpProxyError>;

pub(crate) struct HttpProxyBridge {
    listen_endpoint: ProxyEndpoint,
    socks_endpoint: ProxyEndpoint,
    config: HttpProxyConfig,
}

impl HttpProxyBridge {
    #[must_use]
    pub(crate) const fn new(listen_endpoint: ProxyEndpoint, socks_endpoint: ProxyEndpoint) -> Self {
        Self {
            listen_endpoint,
            socks_endpoint,
            config: HttpProxyConfig { event_sink: None },
        }
    }

    #[must_use]
    pub(crate) fn with_config(mut self, config: HttpProxyConfig) -> Self {
        self.config = config;
        self
    }

    pub(crate) async fn run(self) -> HttpProxyResult<()> {
        let authority = self.listen_endpoint.authority();
        let listener = TcpListener::bind(&authority).await?;
        self.config.emit(HttpProxyEvent::Listening(authority));

        loop {
            let (stream, _) = listener.accept().await?;
            let endpoint = self.socks_endpoint.clone();
            let config = self.config.clone();

            tokio::spawn(async move {
                if let Err(error) = handle_client(stream, endpoint).await {
                    config.emit(HttpProxyEvent::ConnectionFailed(error.to_string()));
                }
            });
        }
    }
}

impl HttpProxyConfig {
    fn emit(&self, event: HttpProxyEvent) {
        if let Some(sink) = &self.event_sink {
            sink(event);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyTarget {
    host: String,
    port: u16,
}

impl fmt::Display for ProxyTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProxyRequest {
    Connect {
        target: ProxyTarget,
        buffered_body: Vec<u8>,
    },
    Forward {
        target: ProxyTarget,
        initial_request: Vec<u8>,
        buffered_body: Vec<u8>,
    },
}

async fn handle_client(
    mut inbound: TcpStream,
    socks_endpoint: ProxyEndpoint,
) -> HttpProxyResult<()> {
    let (headers, buffered_body) = read_http_headers(&mut inbound).await?;
    let request = parse_proxy_request(&headers, buffered_body)?;
    let target = request.target().clone();
    let mut outbound = connect_via_socks(&socks_endpoint, &target).await?;

    match request {
        ProxyRequest::Connect { buffered_body, .. } => {
            inbound
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?;
            if !buffered_body.is_empty() {
                outbound.write_all(&buffered_body).await?;
            }
        }
        ProxyRequest::Forward {
            initial_request,
            buffered_body,
            ..
        } => {
            outbound.write_all(&initial_request).await?;
            if !buffered_body.is_empty() {
                outbound.write_all(&buffered_body).await?;
            }
        }
    }

    if let Err(error) = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await
        && !is_expected_disconnect(&error)
    {
        return Err(error.into());
    }

    Ok(())
}

impl ProxyRequest {
    fn target(&self) -> &ProxyTarget {
        match self {
            Self::Connect { target, .. } | Self::Forward { target, .. } => target,
        }
    }
}

async fn read_http_headers(stream: &mut TcpStream) -> HttpProxyResult<(Vec<u8>, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0; 1024];

    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(HttpProxyError::Protocol(
                "client closed before sending headers".to_string(),
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);

        if let Some(end) = header_end(&bytes) {
            let buffered_body = bytes.split_off(end + 4);
            return Ok((bytes, buffered_body));
        }

        if bytes.len() > MAX_HEADER_BYTES {
            return Err(HttpProxyError::Protocol(
                "request headers are too large".to_string(),
            ));
        }
    }
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_proxy_request(headers: &[u8], buffered_body: Vec<u8>) -> HttpProxyResult<ProxyRequest> {
    let text = str::from_utf8(headers)
        .map_err(|error| HttpProxyError::Protocol(format!("headers are not UTF-8: {error}")))?;
    let (request_line, header_tail) = text
        .split_once("\r\n")
        .ok_or_else(|| HttpProxyError::Protocol("missing request line".to_string()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| HttpProxyError::Protocol("missing HTTP method".to_string()))?;
    let uri = parts
        .next()
        .ok_or_else(|| HttpProxyError::Protocol("missing HTTP target".to_string()))?;
    let version = parts
        .next()
        .ok_or_else(|| HttpProxyError::Protocol("missing HTTP version".to_string()))?;

    if parts.next().is_some() {
        return Err(HttpProxyError::Protocol(
            "request line has too many fields".to_string(),
        ));
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        return Ok(ProxyRequest::Connect {
            target: parse_authority(uri, 443)?,
            buffered_body,
        });
    }

    if let Some((target, origin_form)) = parse_absolute_uri(uri)? {
        let initial_request = format!("{method} {origin_form} {version}\r\n{header_tail}");
        return Ok(ProxyRequest::Forward {
            target,
            initial_request: initial_request.into_bytes(),
            buffered_body,
        });
    }

    let Some(host) = host_header(header_tail) else {
        return Err(HttpProxyError::Protocol(
            "origin-form request is missing Host header".to_string(),
        ));
    };

    Ok(ProxyRequest::Forward {
        target: parse_authority(&host, 80)?,
        initial_request: headers.to_vec(),
        buffered_body,
    })
}

fn parse_absolute_uri(uri: &str) -> HttpProxyResult<Option<(ProxyTarget, String)>> {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Ok(None);
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => {
            return Err(HttpProxyError::Protocol(format!(
                "unsupported absolute URI scheme {scheme}"
            )));
        }
    };
    let split_at = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..split_at];
    let suffix = &rest[split_at..];
    let origin_form = if suffix.is_empty() {
        "/".to_string()
    } else if suffix.starts_with(['?', '#']) {
        format!("/{suffix}")
    } else {
        suffix.to_string()
    };

    Ok(Some((
        parse_authority(authority, default_port)?,
        origin_form,
    )))
}

fn host_header(header_tail: &str) -> Option<String> {
    header_tail.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("host")
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn parse_authority(authority: &str, default_port: u16) -> HttpProxyResult<ProxyTarget> {
    let authority = authority.trim();
    if authority.is_empty() {
        return Err(HttpProxyError::Protocol("empty target host".to_string()));
    }

    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port_part) = rest
            .split_once(']')
            .ok_or_else(|| HttpProxyError::Protocol("invalid IPv6 target".to_string()))?;
        let port = if let Some(port) = port_part.strip_prefix(':') {
            parse_port(port)?
        } else {
            default_port
        };
        (host.to_string(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_string(), parse_port(port)?)
    } else {
        (authority.to_string(), default_port)
    };

    if host.is_empty() {
        return Err(HttpProxyError::Protocol("empty target host".to_string()));
    }

    Ok(ProxyTarget { host, port })
}

fn parse_port(port: &str) -> HttpProxyResult<u16> {
    let port = port
        .parse::<u16>()
        .map_err(|error| HttpProxyError::Protocol(format!("invalid target port: {error}")))?;
    if port == 0 {
        return Err(HttpProxyError::Protocol(
            "target port must not be zero".to_string(),
        ));
    }

    Ok(port)
}

async fn connect_via_socks(
    socks_endpoint: &ProxyEndpoint,
    target: &ProxyTarget,
) -> HttpProxyResult<TcpStream> {
    let mut stream = TcpStream::connect(socks_endpoint.authority()).await?;
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut method_reply = [0; 2];
    stream.read_exact(&mut method_reply).await?;
    if method_reply != [0x05, 0x00] {
        return Err(HttpProxyError::Socks(
            "local SOCKS server rejected no-auth negotiation".to_string(),
        ));
    }

    let host = target.host.as_bytes();
    let host_len = u8::try_from(host.len())
        .map_err(|_| HttpProxyError::Protocol("target host is too long for SOCKS5".to_string()))?;
    let mut request = vec![0x05, 0x01, 0x00, 0x03, host_len];
    request.extend_from_slice(host);
    request.extend_from_slice(&target.port.to_be_bytes());
    stream.write_all(&request).await?;

    let mut reply = [0; 4];
    stream.read_exact(&mut reply).await?;
    if reply[0] != 0x05 {
        return Err(HttpProxyError::Socks(
            "local SOCKS server returned an invalid reply".to_string(),
        ));
    }
    if reply[1] != 0x00 {
        return Err(HttpProxyError::Socks(format!(
            "local SOCKS server could not connect to {target}"
        )));
    }
    drain_socks_bind_address(&mut stream, reply[3]).await?;

    Ok(stream)
}

async fn drain_socks_bind_address(stream: &mut TcpStream, atyp: u8) -> HttpProxyResult<()> {
    match atyp {
        0x01 => {
            let mut addr = [0; 4];
            stream.read_exact(&mut addr).await?;
        }
        0x03 => {
            let len = stream.read_u8().await?;
            let mut addr = vec![0; usize::from(len)];
            stream.read_exact(&mut addr).await?;
        }
        0x04 => {
            let mut addr = [0; 16];
            stream.read_exact(&mut addr).await?;
        }
        unknown => {
            return Err(HttpProxyError::Socks(format!(
                "local SOCKS server returned unsupported address type {unknown}"
            )));
        }
    }

    let mut port = [0; 2];
    stream.read_exact(&mut port).await?;

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

#[cfg(test)]
mod tests {
    use super::{ProxyRequest, parse_absolute_uri, parse_authority, parse_proxy_request};

    #[test]
    fn parses_connect_authority() {
        let target = parse_authority("example.com:443", 443).expect("target parses");

        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 443);
    }

    #[test]
    fn parses_absolute_http_uri() {
        let (target, origin_form) = parse_absolute_uri("http://example.com:8080/path?q=1")
            .expect("uri parses")
            .expect("absolute uri");

        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 8080);
        assert_eq!(origin_form, "/path?q=1");
    }

    #[test]
    fn rewrites_absolute_form_request_to_origin_form() {
        let request = b"GET http://example.com/path HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let parsed = parse_proxy_request(request, Vec::new()).expect("request parses");

        match parsed {
            ProxyRequest::Forward {
                target,
                initial_request,
                ..
            } => {
                assert_eq!(target.host, "example.com");
                assert_eq!(target.port, 80);
                assert!(
                    String::from_utf8(initial_request)
                        .expect("utf8")
                        .starts_with("GET /path HTTP/1.1\r\n")
                );
            }
            ProxyRequest::Connect { .. } => panic!("expected forward request"),
        }
    }

    #[test]
    fn parses_connect_request() {
        let request = b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n";
        let parsed = parse_proxy_request(request, Vec::new()).expect("request parses");

        match parsed {
            ProxyRequest::Connect { target, .. } => {
                assert_eq!(target.host, "example.com");
                assert_eq!(target.port, 443);
            }
            ProxyRequest::Forward { .. } => panic!("expected connect request"),
        }
    }
}

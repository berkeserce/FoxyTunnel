use serde::{Deserialize, Serialize};
use std::{env, fs, path::Path, process::Command};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SystemProxyStatus {
    pub supported: bool,
    pub active: bool,
    pub backend: String,
    pub message: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
}

impl ProxyEndpoint {
    pub(crate) fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn socks_uri(&self) -> String {
        format!("socks://{}", self.authority())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SystemProxyEndpoints {
    pub socks: ProxyEndpoint,
    pub http: ProxyEndpoint,
}

pub(crate) fn platform_status(active: bool, last_error: Option<String>) -> SystemProxyStatus {
    match current_backend() {
        Ok(backend) => SystemProxyStatus {
            supported: true,
            active,
            backend: backend.label().to_string(),
            message: Some(backend.support_message().to_string()),
            last_error,
        },
        Err(message) => SystemProxyStatus {
            supported: false,
            active: false,
            backend: "Unsupported".to_string(),
            message: Some(message),
            last_error,
        },
    }
}

pub(crate) fn is_supported() -> bool {
    current_backend().is_ok()
}

pub(crate) fn apply_system_proxy(
    endpoints: &SystemProxyEndpoints,
    snapshot_path: &Path,
) -> Result<SystemProxyStatus, String> {
    let backend = current_backend()?;

    if snapshot_path.exists() {
        restore_system_proxy_if_owned(snapshot_path)?;
    }

    let snapshot = take_snapshot(backend)?;
    let stored = StoredSnapshot {
        endpoint: endpoints.socks.clone(),
        http_endpoint: Some(endpoints.http.clone()),
        snapshot,
    };
    write_snapshot(snapshot_path, &stored)?;

    if let Err(error) = apply_endpoint(backend, endpoints) {
        let _ = restore_system_proxy(snapshot_path);
        return Err(error);
    }

    Ok(platform_status(true, None))
}

pub(crate) fn restore_system_proxy(snapshot_path: &Path) -> Result<bool, String> {
    restore_snapshot(snapshot_path, false)
}

pub(crate) fn restore_system_proxy_if_owned(snapshot_path: &Path) -> Result<bool, String> {
    restore_snapshot(snapshot_path, true)
}

fn restore_snapshot(snapshot_path: &Path, only_if_owned: bool) -> Result<bool, String> {
    if !snapshot_path.exists() {
        return Ok(false);
    }

    let stored = read_snapshot(snapshot_path)?;
    if only_if_owned && !snapshot_matches_current_endpoint(&stored)? {
        fs::remove_file(snapshot_path)
            .map_err(|error| format!("failed to remove stale proxy snapshot: {error}"))?;
        return Ok(false);
    }

    restore_platform_snapshot(&stored.snapshot)?;
    fs::remove_file(snapshot_path)
        .map_err(|error| format!("failed to remove proxy snapshot: {error}"))?;

    Ok(true)
}

fn write_snapshot(path: &Path, snapshot: &StoredSnapshot) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("failed to create system proxy snapshot directory: {error}")
        })?;
    }

    let contents = serde_json::to_string_pretty(snapshot)
        .map_err(|error| format!("failed to encode system proxy snapshot: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("failed to write system proxy snapshot: {error}"))
}

fn read_snapshot(path: &Path) -> Result<StoredSnapshot, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read system proxy snapshot: {error}"))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse system proxy snapshot: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyBackend {
    #[cfg(target_os = "windows")]
    Windows,
    #[cfg(target_os = "linux")]
    Gnome,
    #[cfg(target_os = "linux")]
    Kde,
}

impl ProxyBackend {
    fn label(self) -> &'static str {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows => "Windows",
            #[cfg(target_os = "linux")]
            Self::Gnome => "GNOME",
            #[cfg(target_os = "linux")]
            Self::Kde => "KDE",
        }
    }

    fn support_message(self) -> &'static str {
        match self {
            #[cfg(target_os = "windows")]
            Self::Windows => "System proxy changes affect proxy-aware Windows applications.",
            #[cfg(target_os = "linux")]
            Self::Gnome | Self::Kde => {
                "System proxy changes affect proxy-aware desktop applications."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSnapshot {
    endpoint: ProxyEndpoint,
    #[serde(default)]
    http_endpoint: Option<ProxyEndpoint>,
    snapshot: PlatformSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "backend", content = "settings", rename_all = "snake_case")]
enum PlatformSnapshot {
    #[cfg(target_os = "windows")]
    Windows(WindowsSnapshot),
    #[cfg(target_os = "linux")]
    Gnome(GnomeSnapshot),
    #[cfg(target_os = "linux")]
    Kde(KdeSnapshot),
}

fn current_backend() -> Result<ProxyBackend, String> {
    current_backend_from_env(
        env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        env::var("DESKTOP_SESSION").ok().as_deref(),
    )
}

#[cfg(target_os = "windows")]
fn current_backend_from_env(
    _xdg_current_desktop: Option<&str>,
    _desktop_session: Option<&str>,
) -> Result<ProxyBackend, String> {
    Ok(ProxyBackend::Windows)
}

#[cfg(target_os = "linux")]
fn current_backend_from_env(
    xdg_current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> Result<ProxyBackend, String> {
    match linux_desktop_from_env(xdg_current_desktop, desktop_session) {
        LinuxDesktop::Kde => {
            if kde_tools().is_some() {
                Ok(ProxyBackend::Kde)
            } else {
                Err("KDE system proxy tools were not found.".to_string())
            }
        }
        LinuxDesktop::Gnome => {
            if command_exists("gsettings") {
                Ok(ProxyBackend::Gnome)
            } else {
                Err("GNOME gsettings was not found.".to_string())
            }
        }
        LinuxDesktop::Unsupported(detected) => Err(format!(
            "System Proxy mode supports GNOME and KDE on Linux; detected {detected}."
        )),
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum LinuxDesktop {
    Kde,
    Gnome,
    Unsupported(String),
}

#[cfg(target_os = "linux")]
fn linux_desktop_from_env(
    xdg_current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> LinuxDesktop {
    let desktop = [xdg_current_desktop, desktop_session]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(":")
        .to_ascii_lowercase();

    if desktop.contains("kde") {
        LinuxDesktop::Kde
    } else if desktop.contains("gnome") {
        LinuxDesktop::Gnome
    } else {
        LinuxDesktop::Unsupported(if desktop.is_empty() {
            "unknown desktop".to_string()
        } else {
            desktop
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn current_backend_from_env(
    _xdg_current_desktop: Option<&str>,
    _desktop_session: Option<&str>,
) -> Result<ProxyBackend, String> {
    Err("System Proxy mode is not implemented for this platform.".to_string())
}

fn take_snapshot(backend: ProxyBackend) -> Result<PlatformSnapshot, String> {
    match backend {
        #[cfg(target_os = "windows")]
        ProxyBackend::Windows => snapshot_windows().map(PlatformSnapshot::Windows),
        #[cfg(target_os = "linux")]
        ProxyBackend::Gnome => snapshot_gnome().map(PlatformSnapshot::Gnome),
        #[cfg(target_os = "linux")]
        ProxyBackend::Kde => snapshot_kde().map(PlatformSnapshot::Kde),
    }
}

fn apply_endpoint(backend: ProxyBackend, endpoints: &SystemProxyEndpoints) -> Result<(), String> {
    match backend {
        #[cfg(target_os = "windows")]
        ProxyBackend::Windows => apply_windows(endpoints),
        #[cfg(target_os = "linux")]
        ProxyBackend::Gnome => apply_gnome(endpoints),
        #[cfg(target_os = "linux")]
        ProxyBackend::Kde => apply_kde(endpoints),
    }
}

fn restore_platform_snapshot(snapshot: &PlatformSnapshot) -> Result<(), String> {
    match snapshot {
        #[cfg(target_os = "windows")]
        PlatformSnapshot::Windows(snapshot) => restore_windows(snapshot),
        #[cfg(target_os = "linux")]
        PlatformSnapshot::Gnome(snapshot) => restore_gnome(snapshot),
        #[cfg(target_os = "linux")]
        PlatformSnapshot::Kde(snapshot) => restore_kde(snapshot),
    }
}

fn snapshot_matches_current_endpoint(stored: &StoredSnapshot) -> Result<bool, String> {
    let endpoints = SystemProxyEndpoints {
        socks: stored.endpoint.clone(),
        http: stored
            .http_endpoint
            .clone()
            .unwrap_or_else(|| stored.endpoint.clone()),
    };

    match &stored.snapshot {
        #[cfg(target_os = "windows")]
        PlatformSnapshot::Windows(_) => windows_matches_endpoint(&endpoints),
        #[cfg(target_os = "linux")]
        PlatformSnapshot::Gnome(_) => gnome_matches_endpoint(&endpoints),
        #[cfg(target_os = "linux")]
        PlatformSnapshot::Kde(_) => kde_matches_endpoint(&endpoints),
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GnomeSnapshot {
    mode: String,
    socks_host: String,
    socks_port: u16,
    #[serde(default)]
    http_host: String,
    #[serde(default)]
    http_port: u16,
    #[serde(default)]
    https_host: String,
    #[serde(default)]
    https_port: u16,
    #[serde(default)]
    ftp_host: String,
    #[serde(default)]
    ftp_port: u16,
}

#[cfg(target_os = "linux")]
fn snapshot_gnome() -> Result<GnomeSnapshot, String> {
    Ok(GnomeSnapshot {
        mode: parse_gvariant_string(&gsettings_get("org.gnome.system.proxy", "mode")?),
        socks_host: parse_gvariant_string(&gsettings_get("org.gnome.system.proxy.socks", "host")?),
        socks_port: parse_u16(
            &gsettings_get("org.gnome.system.proxy.socks", "port")?,
            "GNOME SOCKS port",
        )?,
        http_host: parse_gvariant_string(&gsettings_get("org.gnome.system.proxy.http", "host")?),
        http_port: parse_u16(
            &gsettings_get("org.gnome.system.proxy.http", "port")?,
            "GNOME HTTP proxy port",
        )?,
        https_host: parse_gvariant_string(&gsettings_get("org.gnome.system.proxy.https", "host")?),
        https_port: parse_u16(
            &gsettings_get("org.gnome.system.proxy.https", "port")?,
            "GNOME HTTPS proxy port",
        )?,
        ftp_host: parse_gvariant_string(&gsettings_get("org.gnome.system.proxy.ftp", "host")?),
        ftp_port: parse_u16(
            &gsettings_get("org.gnome.system.proxy.ftp", "port")?,
            "GNOME FTP proxy port",
        )?,
    })
}

#[cfg(target_os = "linux")]
fn apply_gnome(endpoints: &SystemProxyEndpoints) -> Result<(), String> {
    gsettings_set(
        "org.gnome.system.proxy.socks",
        "host",
        &gvariant_string(&endpoints.socks.host),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.socks",
        "port",
        &endpoints.socks.port.to_string(),
    )?;
    clear_gnome_proxy_endpoint("org.gnome.system.proxy.http")?;
    clear_gnome_proxy_endpoint("org.gnome.system.proxy.https")?;
    clear_gnome_proxy_endpoint("org.gnome.system.proxy.ftp")?;
    gsettings_set("org.gnome.system.proxy", "mode", "'manual'")
}

#[cfg(target_os = "linux")]
fn restore_gnome(snapshot: &GnomeSnapshot) -> Result<(), String> {
    gsettings_set(
        "org.gnome.system.proxy.socks",
        "host",
        &gvariant_string(&snapshot.socks_host),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.socks",
        "port",
        &snapshot.socks_port.to_string(),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.http",
        "host",
        &gvariant_string(&snapshot.http_host),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.http",
        "port",
        &snapshot.http_port.to_string(),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.https",
        "host",
        &gvariant_string(&snapshot.https_host),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.https",
        "port",
        &snapshot.https_port.to_string(),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.ftp",
        "host",
        &gvariant_string(&snapshot.ftp_host),
    )?;
    gsettings_set(
        "org.gnome.system.proxy.ftp",
        "port",
        &snapshot.ftp_port.to_string(),
    )?;
    gsettings_set(
        "org.gnome.system.proxy",
        "mode",
        &gvariant_string(&snapshot.mode),
    )
}

#[cfg(target_os = "linux")]
fn gnome_matches_endpoint(endpoints: &SystemProxyEndpoints) -> Result<bool, String> {
    let snapshot = snapshot_gnome()?;

    Ok(snapshot.mode == "manual"
        && snapshot.socks_host == endpoints.socks.host
        && snapshot.socks_port == endpoints.socks.port)
}

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> Result<String, String> {
    run_command("gsettings", &["get", schema, key]).map(|output| output.trim().to_string())
}

#[cfg(target_os = "linux")]
fn gsettings_set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    run_command("gsettings", &["set", schema, key, value]).map(|_| ())
}

#[cfg(target_os = "linux")]
fn clear_gnome_proxy_endpoint(schema: &str) -> Result<(), String> {
    gsettings_set(schema, "host", "''")?;
    gsettings_set(schema, "port", "0")
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct KdeSnapshot {
    proxy_type: Option<String>,
    socks_proxy: Option<String>,
    #[serde(default)]
    http_proxy: Option<String>,
    #[serde(default)]
    https_proxy: Option<String>,
    #[serde(default)]
    ftp_proxy: Option<String>,
    #[serde(default)]
    proxy_config_script: Option<String>,
}

#[cfg(target_os = "linux")]
fn snapshot_kde() -> Result<KdeSnapshot, String> {
    Ok(KdeSnapshot {
        proxy_type: kde_read("ProxyType")?,
        socks_proxy: kde_read("socksProxy")?,
        http_proxy: kde_read("httpProxy")?,
        https_proxy: kde_read("httpsProxy")?,
        ftp_proxy: kde_read("ftpProxy")?,
        proxy_config_script: kde_read("Proxy Config Script")?,
    })
}

#[cfg(target_os = "linux")]
fn apply_kde(endpoints: &SystemProxyEndpoints) -> Result<(), String> {
    let socks_value = kde_proxy_value(&endpoints.socks);

    kde_write("ProxyType", Some("1"))?;
    kde_write("socksProxy", Some(&socks_value))?;
    kde_write("httpProxy", None)?;
    kde_write("httpsProxy", None)?;
    kde_write("ftpProxy", None)?;
    kde_write("Proxy Config Script", None)?;
    refresh_kde_proxy();

    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_kde(snapshot: &KdeSnapshot) -> Result<(), String> {
    kde_write("ProxyType", snapshot.proxy_type.as_deref())?;
    kde_write("socksProxy", snapshot.socks_proxy.as_deref())?;
    kde_write("httpProxy", snapshot.http_proxy.as_deref())?;
    kde_write("httpsProxy", snapshot.https_proxy.as_deref())?;
    kde_write("ftpProxy", snapshot.ftp_proxy.as_deref())?;
    kde_write(
        "Proxy Config Script",
        snapshot.proxy_config_script.as_deref(),
    )?;
    refresh_kde_proxy();

    Ok(())
}

#[cfg(target_os = "linux")]
fn kde_matches_endpoint(endpoints: &SystemProxyEndpoints) -> Result<bool, String> {
    let snapshot = snapshot_kde()?;

    Ok(snapshot.proxy_type.as_deref() == Some("1")
        && snapshot
            .socks_proxy
            .as_deref()
            .is_some_and(|value| socks_proxy_value_matches_endpoint(value, &endpoints.socks)))
}

#[cfg(target_os = "linux")]
fn kde_proxy_value(endpoint: &ProxyEndpoint) -> String {
    endpoint.authority().replace(':', " ")
}

#[cfg(target_os = "linux")]
fn socks_proxy_value_matches_endpoint(value: &str, endpoint: &ProxyEndpoint) -> bool {
    let value = value.trim();
    let colon_form = endpoint.authority();
    let space_form = colon_form.replace(':', " ");
    let accepted_values = [
        colon_form.clone(),
        endpoint.socks_uri(),
        format!("socks5://{colon_form}"),
        space_form.clone(),
        format!("socks://{space_form}"),
        format!("socks5://{space_form}"),
    ];

    accepted_values.iter().any(|accepted| value == accepted)
}

#[cfg(target_os = "linux")]
fn kde_tools() -> Option<(&'static str, &'static str)> {
    if command_exists("kreadconfig6") && command_exists("kwriteconfig6") {
        Some(("kreadconfig6", "kwriteconfig6"))
    } else if command_exists("kreadconfig5") && command_exists("kwriteconfig5") {
        Some(("kreadconfig5", "kwriteconfig5"))
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn kde_read(key: &str) -> Result<Option<String>, String> {
    let Some((reader, _)) = kde_tools() else {
        return Err("KDE proxy tools were not found.".to_string());
    };
    let output = run_command(
        reader,
        &[
            "--file",
            "kioslaverc",
            "--group",
            "Proxy Settings",
            "--key",
            key,
        ],
    )?;
    let value = output.trim().to_string();

    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "linux")]
fn kde_write(key: &str, value: Option<&str>) -> Result<(), String> {
    let Some((_, writer)) = kde_tools() else {
        return Err("KDE proxy tools were not found.".to_string());
    };

    let mut args = vec![
        "--file",
        "kioslaverc",
        "--group",
        "Proxy Settings",
        "--key",
        key,
        "--notify",
    ];
    if let Some(value) = value {
        args.push(value);
    } else {
        args.push("--delete");
    }

    run_command(writer, &args).map(|_| ())
}

#[cfg(target_os = "linux")]
fn refresh_kde_proxy() {
    let _ = run_command(
        "dbus-send",
        &[
            "--type=signal",
            "/KIO/Scheduler",
            "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
            "string:",
        ],
    );
    let _ = run_command(
        "qdbus6",
        &[
            "org.kde.kded6",
            "/modules/proxyscout",
            "org.kde.KPAC.ProxyScout.reset",
        ],
    );
    let _ = run_command(
        "qdbus",
        &[
            "org.kde.kded5",
            "/modules/proxyscout",
            "org.kde.KPAC.ProxyScout.reset",
        ],
    );
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WindowsSnapshot {
    proxy_enable: Option<RegistryValue>,
    proxy_server: Option<RegistryValue>,
    proxy_override: Option<RegistryValue>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RegistryValue {
    kind: String,
    data: String,
}

#[cfg(target_os = "windows")]
const WINDOWS_INTERNET_SETTINGS: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[cfg(target_os = "windows")]
fn snapshot_windows() -> Result<WindowsSnapshot, String> {
    Ok(WindowsSnapshot {
        proxy_enable: registry_read("ProxyEnable")?,
        proxy_server: registry_read("ProxyServer")?,
        proxy_override: registry_read("ProxyOverride")?,
    })
}

#[cfg(target_os = "windows")]
fn apply_windows(endpoints: &SystemProxyEndpoints) -> Result<(), String> {
    registry_write("ProxyEnable", "REG_DWORD", "1")?;
    registry_write("ProxyServer", "REG_SZ", &endpoints.http.authority())?;
    refresh_windows_proxy();

    Ok(())
}

#[cfg(target_os = "windows")]
fn restore_windows(snapshot: &WindowsSnapshot) -> Result<(), String> {
    registry_restore("ProxyEnable", snapshot.proxy_enable.as_ref())?;
    registry_restore("ProxyServer", snapshot.proxy_server.as_ref())?;
    registry_restore("ProxyOverride", snapshot.proxy_override.as_ref())?;
    refresh_windows_proxy();

    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_matches_endpoint(endpoints: &SystemProxyEndpoints) -> Result<bool, String> {
    let proxy_enable = registry_read("ProxyEnable")?;
    let proxy_server = registry_read("ProxyServer")?;
    let enabled = proxy_enable
        .as_ref()
        .is_some_and(|value| registry_dword_enabled(&value.data));
    let http = endpoints.http.authority();
    let old_http = format!("http={http}");
    let old_socks = format!("socks={}", endpoints.socks.authority());

    Ok(enabled
        && proxy_server.as_ref().is_some_and(|value| {
            value.data == http || value.data.contains(&old_http) || value.data == old_socks
        }))
}

#[cfg(target_os = "windows")]
fn registry_read(name: &str) -> Result<Option<RegistryValue>, String> {
    let output = Command::new("reg")
        .args(["query", WINDOWS_INTERNET_SETTINGS, "/v", name])
        .output()
        .map_err(|error| format!("failed to query Windows proxy registry: {error}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 3 && parts[0].eq_ignore_ascii_case(name) {
            return Ok(Some(RegistryValue {
                kind: parts[1].to_string(),
                data: parts[2..].join(" "),
            }));
        }
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn registry_restore(name: &str, value: Option<&RegistryValue>) -> Result<(), String> {
    if let Some(value) = value {
        registry_write(name, &value.kind, &value.data)
    } else {
        let _ = Command::new("reg")
            .args(["delete", WINDOWS_INTERNET_SETTINGS, "/v", name, "/f"])
            .output();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn registry_write(name: &str, kind: &str, data: &str) -> Result<(), String> {
    run_command(
        "reg",
        &[
            "add",
            WINDOWS_INTERNET_SETTINGS,
            "/v",
            name,
            "/t",
            kind,
            "/d",
            data,
            "/f",
        ],
    )
    .map(|_| ())
}

#[cfg(target_os = "windows")]
fn refresh_windows_proxy() {
    let script = r#"
Add-Type -Namespace WinInet -Name NativeMethods -MemberDefinition '[DllImport("wininet.dll", SetLastError=true)] public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);'
[WinInet.NativeMethods]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
[WinInet.NativeMethods]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
"#;
    let _ = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output();
}

fn command_exists(name: &str) -> bool {
    let checker = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    Command::new(checker)
        .arg(name)
        .status()
        .is_ok_and(|status| status.success())
}

fn run_command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run {program}: {error}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{program} failed: {}", stderr.trim()))
    }
}

#[cfg(target_os = "linux")]
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(target_os = "linux")]
fn parse_gvariant_string(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or(value)
        .replace("\\'", "'")
        .replace("\\\\", "\\")
}

#[cfg(target_os = "linux")]
fn parse_u16(value: &str, name: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|error| format!("invalid {name}: {error}"))
}

#[cfg(target_os = "windows")]
fn registry_dword_enabled(value: &str) -> bool {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).is_ok_and(|value| value != 0)
    } else {
        value.parse::<u32>().is_ok_and(|value| value != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyEndpoint, StoredSnapshot};

    #[test]
    fn proxy_endpoint_formats_authority_and_socks_uri() {
        let endpoint = ProxyEndpoint {
            host: "127.0.0.1".to_string(),
            port: 19_050,
        };

        assert_eq!(endpoint.authority(), "127.0.0.1:19050");
        assert_eq!(endpoint.socks_uri(), "socks://127.0.0.1:19050");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn kde_proxy_value_uses_host_space_port_format() {
        use super::kde_proxy_value;

        let endpoint = ProxyEndpoint {
            host: "127.0.0.1".to_string(),
            port: 19_050,
        };

        assert_eq!(kde_proxy_value(&endpoint), "127.0.0.1 19050");
    }

    #[test]
    fn stored_snapshot_requires_endpoint_for_stale_restore_checks() {
        let json = r#"{"endpoint":{"host":"127.0.0.1","port":19050},"snapshot":{"backend":"kde","settings":{"proxy_type":"0","socks_proxy":null}}}"#;
        let snapshot = serde_json::from_str::<StoredSnapshot>(json).expect("snapshot should parse");

        assert_eq!(snapshot.endpoint.authority(), "127.0.0.1:19050");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_backend_detects_supported_desktops() {
        use super::{LinuxDesktop, linux_desktop_from_env};

        assert_eq!(linux_desktop_from_env(Some("KDE"), None), LinuxDesktop::Kde);
        assert_eq!(
            linux_desktop_from_env(Some("GNOME"), None),
            LinuxDesktop::Gnome
        );
        assert_eq!(
            linux_desktop_from_env(Some("sway"), None),
            LinuxDesktop::Unsupported("sway".to_string())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gvariant_strings_roundtrip_simple_values() {
        use super::{gvariant_string, parse_gvariant_string};

        assert_eq!(gvariant_string("manual"), "'manual'");
        assert_eq!(parse_gvariant_string("'127.0.0.1'"), "127.0.0.1");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn kde_proxy_value_matching_accepts_common_socks_formats() {
        use super::socks_proxy_value_matches_endpoint;

        let endpoint = ProxyEndpoint {
            host: "127.0.0.1".to_string(),
            port: 19_050,
        };

        assert!(socks_proxy_value_matches_endpoint(
            "127.0.0.1:19050",
            &endpoint
        ));
        assert!(socks_proxy_value_matches_endpoint(
            "socks://127.0.0.1:19050",
            &endpoint
        ));
        assert!(socks_proxy_value_matches_endpoint(
            "socks5://127.0.0.1:19050",
            &endpoint
        ));
        assert!(socks_proxy_value_matches_endpoint(
            "127.0.0.1 19050",
            &endpoint
        ));
        assert!(!socks_proxy_value_matches_endpoint(
            "127.0.0.1:19051",
            &endpoint
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_dword_enabled_parses_hex_and_decimal() {
        use super::registry_dword_enabled;

        assert!(registry_dword_enabled("0x1"));
        assert!(registry_dword_enabled("1"));
        assert!(!registry_dword_enabled("0x0"));
        assert!(!registry_dword_enabled("0"));
    }
}

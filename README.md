# FoxyTunnel

FoxyTunnel is a Windows-first Rust application for routing supported traffic
through the Tor network.

The project is in early development. The first target is a tray-based Windows
app with:

- an in-process Arti-powered Tor client,
- a local SOCKS5 endpoint,
- Wintun/tun2proxy-based tunnel mode,
- a small application-based proxy launch mode,
- clear leak-protection boundaries for DNS and UDP.

## Status

This repository is being scaffolded. It is not ready for daily use yet.

## Security and privacy expectations

FoxyTunnel should avoid promising more protection than it can provide.

The initial tunnel mode is expected to route TCP traffic through Tor. DNS must
be protected explicitly, and UDP/ICMP support is either blocked or out of scope
until implemented and tested.

## Development

Install a recent Rust stable toolchain and the Microsoft Visual C++ build tools.

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run the temporary CLI entry point:

```powershell
cargo run -p foxytunnel-app
cargo run -p foxytunnel-app -- --bootstrap
cargo run -p foxytunnel-app -- --socks
cargo run -p foxytunnel-app -- --socks --config foxytunnel.toml
cargo run -p foxytunnel-app -- --socks --port 19051 --log
```

The `--socks` command starts a local SOCKS5 proxy after Tor bootstrap succeeds.
The default endpoint is `127.0.0.1:19050`. Use `--port` to change the port and
`--log` to print accepted SOCKS CONNECT targets.

Run the desktop GUI:

```powershell
cd apps/desktop
npm install
npm run tauri dev
```

The first GUI screen can start and stop the local SOCKS proxy, set the SOCKS
port, change the bootstrap timeout, and enable connection logging.

Example `foxytunnel.toml`:

```toml
socks_host = "127.0.0.1"
socks_port = 19050
log_connections = false
bootstrap_timeout_seconds = 120
arti_state_dir = "target/foxytunnel/arti-state"
arti_cache_dir = "target/foxytunnel/arti-cache"
```

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.

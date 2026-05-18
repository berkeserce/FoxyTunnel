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

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.

# Architecture

FoxyTunnel is split into small Rust crates so that Tor, tunnel, and UI concerns
stay separate.

## Crates

- `foxytunnel-core`: Tor client state, local proxy state, shared configuration.
- `foxytunnel-tunnel`: Windows tunnel planning and future Wintun/tun2proxy code.
- `foxytunnel-app`: application entry point, currently a placeholder before the
  Tauri tray shell is added.

## Planned runtime flow

1. The app starts the core service.
2. The core service bootstraps Arti and exposes a local SOCKS5 endpoint.
3. Tunnel mode creates or opens a Wintun adapter.
4. tun2proxy forwards supported TCP flows to the local SOCKS5 endpoint.
5. DNS and UDP are handled according to explicit leak-protection policy.

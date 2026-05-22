# Architecture

FoxyTunnel is split into small Rust crates so that Tor, tunnel, and UI concerns
stay separate.

## Crates

- `foxytunnel-core`: Tor client state, local proxy state, shared configuration.
- `foxytunnel-tunnel`: cross-platform tunnel planning and OS-specific backend
  boundary.
- `foxytunnel-app`: application entry point, currently a placeholder before the
  Tauri tray shell is added.

## Platform backends

Tunnel mode is split behind a shared `TunnelBackend` trait. Linux and Windows
builds expose a `PlatformTunnelBackend` alias so application code can depend on
one backend name while implementation details stay behind `cfg` gates.

- Linux backend: future TUN device, policy routing, DNS policy, and UDP/ICMP
  handling.
- Windows backend: future Wintun/tun2proxy integration, route updates, DNS
  policy, and UDP/ICMP handling.

## Planned runtime flow

1. The app starts the core service.
2. The core service bootstraps Arti and exposes a local SOCKS5 endpoint.
3. Tunnel mode starts the current platform backend.
4. The platform backend forwards supported TCP flows to the local SOCKS5
   endpoint.
5. DNS and UDP are handled according to explicit leak-protection policy.

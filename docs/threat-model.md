# Threat Model

FoxyTunnel should make conservative privacy claims.

## In scope for the first usable tunnel mode

- TCP traffic routed through Tor.
- Local SOCKS5 endpoint bound to loopback.
- DNS leak prevention.
- Clear failure states when Tor bootstrap or tunnel setup fails.

## Out of scope until explicitly implemented

- UDP tunneling through Tor.
- ICMP tunneling.
- Protection from malware running as the same user.
- Protection from a compromised operating system.
- Claims of complete anonymity.

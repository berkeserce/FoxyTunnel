# Contributing

FoxyTunnel uses small, logical commits and Conventional Commits.

## Commit style

Use English commit messages:

```text
feat(core): add arti bootstrap state
fix(tunnel): restore routes after startup failure
docs: document dns and udp limitations
ci: run rust checks
chore: scaffold workspace
```

Recommended scopes:

- `app`
- `ci`
- `core`
- `docs`
- `release`
- `tunnel`
- `ui`

## Before committing

Run:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

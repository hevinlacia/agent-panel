# Deployment — systemd user service

Run `agent-panel` as a long-lived **Rust + React** local web service using a systemd user unit.

## TL;DR

```bash
cd ~/Developer/tools/agent-panel
./scripts/install-systemd.sh
# open http://localhost:7331
```

The installer:

1. resolves `bun` and `cargo`,
2. runs `bun install`,
3. builds the React SPA into `public/dashboard-react/`,
4. builds the Rust backend into `target/release/agent-panel`,
5. writes `~/.config/systemd/user/agent-panel.service`,
6. enables and restarts the service.

## Requirements

- Linux with systemd user services.
- Bun for the React build.
- Rust/Cargo for the Axum backend.

## Customization

| Var | Default | Purpose |
| --- | --- | --- |
| `PORT` | `7331` | HTTP port |
| `BUN_BIN` | auto-detected | Bun executable |
| `CARGO_BIN` | auto-detected | Cargo executable |

Examples:

```bash
PORT=8080 ./scripts/install-systemd.sh
BUN_BIN=/usr/bin/bun CARGO_BIN=/usr/bin/cargo ./scripts/install-systemd.sh
```

## Common commands

```bash
systemctl --user status   agent-panel
systemctl --user restart  agent-panel
systemctl --user stop     agent-panel
journalctl --user -u agent-panel -f
tail -f ~/.local/state/agent-panel.log
```

## Updating after `git pull`

```bash
cd ~/Developer/tools/agent-panel
bun install
bun run build
systemctl --user restart agent-panel
```

## Files written outside the repo

| Path | Purpose |
| --- | --- |
| `~/.config/systemd/user/agent-panel.service` | rendered unit file |
| `~/.local/state/agent-panel.log` | stdout/stderr |
| `~/.local/share/agent-panel/config.json` | local dashboard config |
| `~/.local/share/agent-panel/associations.json` | requirement ↔ session associations |
| `~/.local/share/agent-panel/ctx/*.md` | generated pi session context files |

## Uninstall

```bash
./scripts/uninstall-systemd.sh
./scripts/uninstall-systemd.sh --purge
```

The uninstall script uses `trash-put` when available so the unit/log can be restored from the desktop trash.

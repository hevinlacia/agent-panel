# Deployment — systemd user service

Run `agent-panel` as a long-lived background service on Linux using
**systemd user units** (no root required).

## TL;DR

```bash
cd ~/GitHub/agent-panel
./scripts/install-systemd.sh

# open http://localhost:7331
```

The script:

1. resolves a `node` binary (priority: `$NODE_BIN` > `mise which node` > `PATH`),
2. runs `npm install` if `node_modules/` is missing,
3. renders `scripts/agent-panel.service` into
   `~/.config/systemd/user/agent-panel.service`,
4. `daemon-reload`, `enable`, `restart`,
5. prints `systemctl status`.

Rerunning is safe — it just overwrites the unit file and restarts.

## Requirements

- Linux with systemd (user instance enabled — default on most distros).
- Node.js (`mise`, `nvm`, system package — any works).
- This repo cloned locally.

## Customization

Environment variables understood by `install-systemd.sh`:

| Var        | Default                                   | Purpose                          |
| ---------- | ----------------------------------------- | -------------------------------- |
| `PORT`     | `7331`                                    | HTTP port                        |
| `NODE_BIN` | auto-detected (`mise` → `PATH`)           | Absolute path to `node`          |

Examples:

```bash
PORT=8080 ./scripts/install-systemd.sh
NODE_BIN=/usr/bin/node ./scripts/install-systemd.sh
```

Changing the port after install: rerun the script with the new `PORT=`.

## Common commands

```bash
systemctl --user status   agent-panel      # state + last log lines
systemctl --user restart  agent-panel      # apply code changes
systemctl --user stop     agent-panel
systemctl --user start    agent-panel
systemctl --user disable  agent-panel      # stop auto-start on login
journalctl --user -u agent-panel -f        # systemd-side logs
tail -f ~/.local/state/agent-panel.log     # stdout / stderr
```

## Start on boot without login

By default a user service starts when you log in. To make it run from system
boot (e.g. on a headless machine), enable lingering once:

```bash
sudo loginctl enable-linger "$USER"
```

Disable later with `sudo loginctl disable-linger "$USER"`.

## Updating after `git pull`

```bash
cd ~/GitHub/agent-panel
git pull
npm install                                  # only if deps changed
systemctl --user restart agent-panel
```

If `package.json` added a native dependency (e.g. `node-pty`), npm may print
`allow-scripts` warnings. Approve them so the native binding is built:

```bash
npm approve-scripts <pkg>
```

Then rerun `npm install` and restart the service.

## Files written outside the repo

| Path                                                       | Purpose                          |
| ---------------------------------------------------------- | -------------------------------- |
| `~/.config/systemd/user/agent-panel.service`        | Rendered unit file               |
| `~/.local/state/agent-panel.log`                    | Application stdout/stderr        |
| `~/.config/systemd/user/default.target.wants/...` (symlink)| Auto-start on user login         |

Nothing under `/etc` or `/usr` is touched. No sudo is required (unless you opt
in to `enable-linger`).

## Uninstall

```bash
./scripts/uninstall-systemd.sh           # keep the log
./scripts/uninstall-systemd.sh --purge   # delete the log too
```

## Troubleshooting

- **`status` shows `Active: failed`**
  Check `journalctl --user -u agent-panel --since '5 min ago'` and the
  app log. Most common cause: port in use (`ss -ltnp | grep 7331`) or `node`
  path stale after a Node version switch — rerun `install-systemd.sh`.

- **`could not locate node`**
  Pass an explicit path: `NODE_BIN=$(which node) ./scripts/install-systemd.sh`.

- **`node-pty` errors at startup**
  Native binding missing. Run `npm rebuild node-pty` in the repo, then restart
  the service.

- **Service started but `curl http://localhost:7331` fails**
  Check the bound port from the log (`Agent Panel running at ...`); it
  reflects the `PORT` env, not necessarily `7331`.

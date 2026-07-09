#!/usr/bin/env bash
# Install agent-panel as a systemd user service.
#
# Usage:
#   ./scripts/install-systemd.sh                 # use defaults
#   PORT=8080 ./scripts/install-systemd.sh       # custom port
#   BUN_BIN=/usr/bin/bun ./scripts/install-systemd.sh
#
# Idempotent: rerunning replaces the unit file and restarts the service.

set -euo pipefail

SERVICE_NAME="agent-panel.service"
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="${REPO_DIR}/scripts/agent-panel.service"
UNIT_DIR="${HOME}/.config/systemd/user"
UNIT_FILE="${UNIT_DIR}/${SERVICE_NAME}"
LOG_DIR="${HOME}/.local/state"
LOG_FILE="${LOG_DIR}/agent-panel.log"
PORT="${PORT:-7331}"

# Resolve bun binary. Priority: $BUN_BIN > mise > PATH.
resolve_bun() {
  if [[ -n "${BUN_BIN:-}" && -x "${BUN_BIN}" ]]; then
    echo "${BUN_BIN}"
    return
  fi
  if command -v mise >/dev/null 2>&1; then
    local p
    p="$(mise which bun 2>/dev/null || true)"
    if [[ -n "${p}" && -x "${p}" ]]; then
      echo "${p}"
      return
    fi
  fi
  if command -v bun >/dev/null 2>&1; then
    command -v bun
    return
  fi
  echo "ERROR: could not locate bun. Set BUN_BIN=/path/to/bun." >&2
  exit 1
}

BUN_BIN="$(resolve_bun)"
BUN_BIN_DIR="$(dirname "${BUN_BIN}")"

echo ">> repo:      ${REPO_DIR}"
echo ">> bun:       ${BUN_BIN}"
echo ">> port:      ${PORT}"
echo ">> log file:  ${LOG_FILE}"
echo ">> unit file: ${UNIT_FILE}"

# Sanity: dependencies installed.
if [[ ! -f "${REPO_DIR}/node_modules/tsx/dist/cli.mjs" ]]; then
  echo ">> node_modules not found, running bun install..."
  (cd "${REPO_DIR}" && "${BUN_BIN}" install)
fi

mkdir -p "${UNIT_DIR}" "${LOG_DIR}"

# Render template (use | as sed delimiter; paths contain /).
sed \
  -e "s|__WORKDIR__|${REPO_DIR}|g" \
  -e "s|__BUN_BIN__|${BUN_BIN}|g" \
  -e "s|__BUN_BIN_DIR__|${BUN_BIN_DIR}|g" \
  -e "s|__PORT__|${PORT}|g" \
  -e "s|__LOG_FILE__|${LOG_FILE}|g" \
  "${TEMPLATE}" > "${UNIT_FILE}"

systemctl --user daemon-reload
systemctl --user enable "${SERVICE_NAME}" >/dev/null
systemctl --user restart "${SERVICE_NAME}"

# Brief wait, then status.
sleep 1
systemctl --user status "${SERVICE_NAME}" --no-pager | head -n 12 || true

echo
echo "Installed. Useful commands:"
echo "  systemctl --user status   ${SERVICE_NAME}"
echo "  systemctl --user restart  ${SERVICE_NAME}"
echo "  systemctl --user stop     ${SERVICE_NAME}"
echo "  journalctl --user -u ${SERVICE_NAME} -f"
echo "  tail -f ${LOG_FILE}"
echo
echo "Open: http://localhost:${PORT}"
echo
echo "Tip: to start on boot without login, run once:"
echo "  sudo loginctl enable-linger \$USER"

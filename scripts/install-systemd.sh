#!/usr/bin/env bash
# Install agent-panel as a systemd user service.
#
# Usage:
#   ./scripts/install-systemd.sh                 # use defaults
#   PORT=8080 ./scripts/install-systemd.sh       # custom port
#   NODE_BIN=/usr/bin/node ./scripts/install-systemd.sh
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

# Resolve node binary. Priority: $NODE_BIN > mise > PATH.
resolve_node() {
  if [[ -n "${NODE_BIN:-}" && -x "${NODE_BIN}" ]]; then
    echo "${NODE_BIN}"
    return
  fi
  if command -v mise >/dev/null 2>&1; then
    local p
    p="$(mise which node 2>/dev/null || true)"
    if [[ -n "${p}" && -x "${p}" ]]; then
      echo "${p}"
      return
    fi
  fi
  if command -v node >/dev/null 2>&1; then
    command -v node
    return
  fi
  echo "ERROR: could not locate node. Set NODE_BIN=/path/to/node." >&2
  exit 1
}

NODE_BIN="$(resolve_node)"
NODE_BIN_DIR="$(dirname "${NODE_BIN}")"

echo ">> repo:      ${REPO_DIR}"
echo ">> node:      ${NODE_BIN}"
echo ">> port:      ${PORT}"
echo ">> log file:  ${LOG_FILE}"
echo ">> unit file: ${UNIT_FILE}"

# Sanity: dependencies installed.
if [[ ! -f "${REPO_DIR}/node_modules/tsx/dist/cli.mjs" ]]; then
  echo ">> node_modules not found, running npm install..."
  (cd "${REPO_DIR}" && "${NODE_BIN_DIR}/npm" install)
fi

mkdir -p "${UNIT_DIR}" "${LOG_DIR}"

# Render template (use | as sed delimiter; paths contain /).
sed \
  -e "s|__WORKDIR__|${REPO_DIR}|g" \
  -e "s|__NODE_BIN__|${NODE_BIN}|g" \
  -e "s|__NODE_BIN_DIR__|${NODE_BIN_DIR}|g" \
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

#!/usr/bin/env bash
# Install agent-panel as a systemd user service.
#
# Usage:
#   ./scripts/install-systemd.sh
#   PORT=8080 ./scripts/install-systemd.sh
#   BUN_BIN=/usr/bin/bun CARGO_BIN=/usr/bin/cargo ./scripts/install-systemd.sh

set -euo pipefail

SERVICE_NAME="agent-panel.service"
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="${REPO_DIR}/scripts/agent-panel.service"
UNIT_DIR="${HOME}/.config/systemd/user"
UNIT_FILE="${UNIT_DIR}/${SERVICE_NAME}"
LOG_DIR="${HOME}/.local/state"
LOG_FILE="${LOG_DIR}/agent-panel.log"
PORT="${PORT:-7331}"

resolve_bin() {
  local env_name="$1" tool="$2"
  local override="${!env_name:-}"
  if [[ -n "${override}" && -x "${override}" ]]; then
    echo "${override}"
    return
  fi
  if command -v mise >/dev/null 2>&1; then
    local p
    p="$(mise which "${tool}" 2>/dev/null || true)"
    if [[ -n "${p}" && -x "${p}" ]]; then
      echo "${p}"
      return
    fi
  fi
  if command -v "${tool}" >/dev/null 2>&1; then
    command -v "${tool}"
    return
  fi
  echo "ERROR: could not locate ${tool}. Set ${env_name}=/path/to/${tool}." >&2
  exit 1
}

BUN_BIN="$(resolve_bin BUN_BIN bun)"
CARGO_BIN="$(resolve_bin CARGO_BIN cargo)"
BACKEND_BIN="${REPO_DIR}/target/release/agent-panel"

printf '>> repo:      %s\n' "${REPO_DIR}"
printf '>> bun:       %s\n' "${BUN_BIN}"
printf '>> cargo:     %s\n' "${CARGO_BIN}"
printf '>> binary:    %s\n' "${BACKEND_BIN}"
printf '>> port:      %s\n' "${PORT}"
printf '>> log file:  %s\n' "${LOG_FILE}"
printf '>> unit file: %s\n' "${UNIT_FILE}"

mkdir -p "${UNIT_DIR}" "${LOG_DIR}"

(cd "${REPO_DIR}" && "${BUN_BIN}" install)
(cd "${REPO_DIR}" && "${BUN_BIN}" run build:dashboard)
(cd "${REPO_DIR}" && "${CARGO_BIN}" build --release)

sed \
  -e "s|__WORKDIR__|${REPO_DIR}|g" \
  -e "s|__BIN__|${BACKEND_BIN}|g" \
  -e "s|__PORT__|${PORT}|g" \
  -e "s|__LOG_FILE__|${LOG_FILE}|g" \
  "${TEMPLATE}" > "${UNIT_FILE}"

systemctl --user daemon-reload
systemctl --user enable "${SERVICE_NAME}" >/dev/null
systemctl --user restart "${SERVICE_NAME}"

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

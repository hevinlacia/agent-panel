#!/usr/bin/env bash
# Uninstall the agent-panel systemd user service.
#
# Keeps the log file by default. Pass --purge to move it to trash too.

set -euo pipefail

SERVICE_NAME="agent-panel.service"
UNIT_FILE="${HOME}/.config/systemd/user/${SERVICE_NAME}"
LOG_FILE="${HOME}/.local/state/agent-panel.log"

trash_or_remove() {
  local path="$1"
  [[ -e "${path}" ]] || return 0
  if command -v trash-put >/dev/null 2>&1; then
    trash-put "${path}"
  else
    rm -f "${path}"
  fi
}

if systemctl --user list-unit-files 2>/dev/null | grep -q "^${SERVICE_NAME}"; then
  systemctl --user disable --now "${SERVICE_NAME}" || true
fi

trash_or_remove "${UNIT_FILE}"
systemctl --user daemon-reload

if [[ "${1:-}" == "--purge" ]]; then
  trash_or_remove "${LOG_FILE}"
  echo "Removed unit file and moved log to trash when possible."
else
  echo "Removed unit file. Log kept at ${LOG_FILE} (pass --purge to move it to trash too)."
fi

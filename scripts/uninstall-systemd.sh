#!/usr/bin/env bash
# Uninstall the opencode-dashboard systemd user service.
#
# Keeps the log file by default. Pass --purge to remove it as well.

set -euo pipefail

SERVICE_NAME="opencode-dashboard.service"
UNIT_FILE="${HOME}/.config/systemd/user/${SERVICE_NAME}"
LOG_FILE="${HOME}/.local/state/opencode-dashboard.log"

if systemctl --user list-unit-files 2>/dev/null | grep -q "^${SERVICE_NAME}"; then
  systemctl --user disable --now "${SERVICE_NAME}" || true
fi

rm -f "${UNIT_FILE}"
systemctl --user daemon-reload

if [[ "${1:-}" == "--purge" ]]; then
  rm -f "${LOG_FILE}"
  echo "Removed unit file and log."
else
  echo "Removed unit file. Log kept at ${LOG_FILE} (pass --purge to delete)."
fi

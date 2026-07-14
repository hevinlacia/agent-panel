#!/usr/bin/env bash
set -uo pipefail

# hot-deploy-agent-panel.sh - Blue/green deploy agent-panel without dropping users.
#
# Flow: build dashboard -> start inactive backend -> wait /health -> atomically
# switch active-backend.json -> stop old backend (it drains live WebSockets up
# to 5 min via TimeoutStopSec=300). The front proxy (agent-panel-proxy.service)
# never stops and routes new connections to the active slot. Only the backend
# holding scheduler.lock runs background schedulers, so a deploy never double-
# runs sync/extract workers.

STATE_DIR="$HOME/.local/state/agent-panel"
ACTIVE_FILE="$STATE_DIR/active-backend.json"
PROXY_SERVICE="agent-panel-proxy.service"
WORKDIR="$HOME/Developer/tools/agent-panel"
BUN="${BUN:-/home/hevin/.local/share/mise/installs/bun/latest/bin/bun}"

slot_url() { case "$1" in blue) echo "http://127.0.0.1:7332" ;; green) echo "http://127.0.0.1:7333" ;; esac; }
slot_service() { echo "agent-panel-backend@$1.service"; }

read_active_slot() {
  [[ -f "$ACTIVE_FILE" ]] || { echo ""; return; }
  grep -o '"slot":"[^"]*"' "$ACTIVE_FILE" 2>/dev/null | head -1 | cut -d'"' -f4
}

inactive_slot() { [[ "$1" == "blue" ]] && echo green || echo blue; }

is_active_svc() { [[ "$(systemctl --user is-active "$1" 2>/dev/null)" == "active" ]]; }

health_url() {
  curl -sf -o /dev/null --max-time 3 "${1%/}/health" 2>/dev/null
}

wait_healthy() {
  local slot="$1" timeout="${2:-90}" url
  url=$(slot_url "$slot")
  local deadline=$(( $(date +%s) + timeout ))
  while (( $(date +%s) < deadline )); do
    health_url "$url" && return 0
    sleep 1
  done
  echo "ERROR: backend $slot not healthy at $url/health within ${timeout}s" >&2
  return 1
}

write_active_slot() {
  local slot="$1" url
  url=$(slot_url "$slot")
  mkdir -p "$STATE_DIR"
  local tmp="$ACTIVE_FILE.$$.tmp"
  printf '{"slot":"%s","base_url":"%s","updated_at":%d}\n' "$slot" "$url" "$(date +%s)" > "$tmp"
  mv "$tmp" "$ACTIVE_FILE"
}

build_dashboard() {
  echo "building dashboard bundle..."
  (cd "$WORKDIR" && "$BUN" run build:dashboard) || { echo "ERROR: build:dashboard failed" >&2; return 1; }
}

cmd_status() {
  local active
  active=$(read_active_slot)
  echo "active_slot=${active:-unset}"
  for slot in blue green; do
    local url; url=$(slot_url "$slot")
    printf "%-6s service=%-10s health=%-4s url=%s\n" "$slot" \
      "$(is_active_svc "$(slot_service "$slot")" && echo active || echo inactive)" \
      "$(health_url "$url" && echo ok || echo fail)" "$url"
  done
  printf "proxy  service=%s\n" "$(is_active_svc "$PROXY_SERVICE" && echo active || echo inactive)"
  echo "active_file=$ACTIVE_FILE"
}

cmd_bootstrap() {
  local slot="${1:-blue}" timeout="${2:-90}"
  build_dashboard || return 1
  systemctl --user reset-failed "$(slot_service "$slot")" 2>/dev/null || true
  systemctl --user start "$(slot_service "$slot")"
  wait_healthy "$slot" "$timeout" || return 1
  write_active_slot "$slot"
  systemctl --user enable --now "$PROXY_SERVICE"
  echo "bootstrapped $slot"
  cmd_status
}

cmd_deploy() {
  local target="${1:-}" timeout="${2:-90}"
  local active old
  active=$(read_active_slot)
  [[ -z "$target" ]] && target=$(inactive_slot "$active")
  old="$active"
  [[ "$old" == "$target" ]] && old=""

  echo "current=${active:-unset} target=$target old=${old:-none}"
  build_dashboard || return 1

  systemctl --user enable --now "$PROXY_SERVICE" 2>/dev/null || true
  systemctl --user reset-failed "$(slot_service "$target")" 2>/dev/null || true
  systemctl --user start "$(slot_service "$target")"
  wait_healthy "$target" "$timeout" || return 1

  write_active_slot "$target"
  echo "switched active backend to $target"

  if [[ -n "$old" ]]; then
    echo "stopping old backend $old (SIGTERM; drains WS up to 5min)..."
    systemctl --user stop "$(slot_service "$old")"
    echo "stopped old backend $old"
  fi
  cmd_status
}

usage() {
  sed -n '2,/^$/p' "$0"
  cat <<'EOF'

Usage:
  hot-deploy-agent-panel.sh status
  hot-deploy-agent-panel.sh bootstrap [--slot blue|green] [--health-timeout N]
  hot-deploy-agent-panel.sh deploy    [--slot blue|green] [--health-timeout N]
EOF
}

main() {
  local cmd="${1:-}"
  [[ $# -gt 0 ]] && shift
  local slot="" timeout=90
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --slot) slot="$2"; shift 2 ;;
      --health-timeout) timeout="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
  done
  case "$cmd" in
    status) cmd_status ;;
    bootstrap) cmd_bootstrap "$slot" "$timeout" ;;
    deploy) cmd_deploy "$slot" "$timeout" ;;
    *) usage; exit 1 ;;
  esac
}

main "$@"

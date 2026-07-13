#!/usr/bin/env bash
# current-session.sh - 查询最近活跃的 agent session ID（dashboard 视角）。
#
# 优先级：
#   1. Dashboard API (/api/sessions) - 跨 harness，返回 pi 的 UUID 或 opencode 的 ses_xxx
#   2. SQLite 直查 opencode.db - opencode harness 回退
#   3. opencode CLI - opencode harness 最后回退
#
# 注意：本脚本返回 dashboard 列表中"最近活跃"的 session，不一定是当前进程的
# session（多 session 并发活跃时可能取到别的）。若在 pi 会话内，优先调用
# get_session_info 工具获取确定性的当前 session id，本脚本仅作回退。
#
# 输出：session id 字符串，或 UNKNOWN
set -euo pipefail

DASH="${DASHBOARD_URL:-http://localhost:7331}"
DB_PATH="${OPENCODE_DB:-$HOME/.local/share/opencode/opencode.db}"

# Method 1: Dashboard API（跨 harness，pi 下返回 UUID）
SID=$(curl -sf --max-time 3 "$DASH/api/sessions" 2>/dev/null \
  | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    sessions = data.get('sessions', [])
    if sessions:
        sid = sessions[0].get('id', '')
        if sid:
            print(sid)
except: pass
" 2>/dev/null || true)

if [[ -n "$SID" ]]; then
  echo "$SID"
  exit 0
fi

# Method 2: SQLite 直查（opencode harness 回退，pi 不写此库）
if command -v sqlite3 >/dev/null 2>&1 && [[ -f "$DB_PATH" ]]; then
  SID=$(sqlite3 "$DB_PATH" \
    "SELECT id FROM session WHERE time_archived IS NULL ORDER BY time_updated DESC LIMIT 1;" 2>/dev/null || true)
  if [[ -n "$SID" ]]; then
    echo "$SID"
    exit 0
  fi
fi

# Method 3: opencode CLI（opencode harness 回退）
if command -v opencode >/dev/null 2>&1; then
  SID=$(opencode session list --format json --max-count 1 2>/dev/null \
    | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    if isinstance(data, list) and data:
        sid = data[0].get('id', '')
        if sid:
            print(sid)
except: pass
" 2>/dev/null || true)
  if [[ -n "$SID" ]]; then
    echo "$SID"
    exit 0
  fi
fi

echo "UNKNOWN"
exit 1

#!/usr/bin/env bash
# 容器内数据目录自动快照：catch-up 远端 + git add -A + commit + push。
# 由 systemd timer（hub-data-autocommit.timer）每 15 分钟调用一次。
# 用法：hub-data-autocommit.sh <data-repo-dir>
# 2026-10-20：拆分独立数据仓 agent-panel-data；fetch/rebase 失败不阻塞本地快照。
set -euo pipefail

DATA_DIR="${1:?usage: hub-data-autocommit.sh <data-repo-dir>}"
BRANCH="${DATA_BRANCH:-main}"

cd "$DATA_DIR"

# 追赶远端：fetch 失败（无鉴权/断网）只告警；rebase 冲突则放弃、留在本地基线上继续快照
if git remote get-url origin >/dev/null 2>&1 && git fetch -q origin "$BRANCH" 2>/dev/null; then
  if ! git rebase -q FETCH_HEAD >/dev/null 2>&1; then
    git rebase --abort >/dev/null 2>&1 || true
    echo "warn: rebase onto origin/$BRANCH failed, snapshotting on local base" >&2
  fi
else
  echo "warn: origin fetch skipped (no remote/auth/network), snapshotting locally" >&2
fi

git add -A
if git diff --cached --quiet; then
  exit 0
fi
git -c user.name=agent-panel-hub -c user.email=agent-panel-hub@localhost \
  commit -m "data: auto snapshot $(date '+%F %T')"
git push origin "$BRANCH"

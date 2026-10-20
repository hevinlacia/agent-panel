#!/usr/bin/env bash
# 容器内数据目录自动快照：git add -A + commit + push。
# 由 systemd timer（hub-data-autocommit.timer）每 15 分钟调用一次。
# 用法：hub-data-autocommit.sh <data-repo-dir>
set -euo pipefail

DATA_DIR="${1:?usage: hub-data-autocommit.sh <data-repo-dir>}"
BRANCH="${DATA_BRANCH:-main}"

cd "$DATA_DIR"
git add -A
if git diff --cached --quiet; then
  exit 0
fi
git -c user.name=agent-panel-hub -c user.email=agent-panel-hub@localhost \
  commit -m "data: auto snapshot $(date '+%F %T')"
git push origin "$BRANCH"

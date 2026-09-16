#!/usr/bin/env python3
"""Migrate legacy requirement-group req ids to the WMS-GRP- numbering pool.

One-off (re-runnable) migration for Agent Panel requirement directories:

- Scans `requirementScanRoots` from ~/.local/share/agent-panel/config.json
  (falls back to the two known roots) for requirement dirs that contain a
  group.json (the 引用式需求组 identity marker) and still use the legacy
  normal-requirement pool form `WMS-<seq>-<slug>`.
- Allocates fresh ids from the WMS-GRP- pool (`WMS-GRP-<seq:03d>-<slug>`,
  max existing GRP seq + 1) and renames those dirs.
- Rewrites the old full id to the new full id inside every text file of the
  renamed dir (meta.md / state.json / group.json / notes.md / events.jsonl /
  ...).
- Rewrites cross references in ALL other requirement dirs under the scan
  roots (notes, events, group.json members never reference groups, but text
  mentions are rewritten anyway).
- Rewrites runtime data under ~/.local/share/agent-panel/ (*.json/*.jsonl
  mentioning the old full id, e.g. associations.json keys).

Dirs already using WMS-GRP- (or non-standard ids) are skipped. Dry-run by
default; pass --execute to apply. Idempotent: re-running skips dirs already
migrated and replacements that no longer match.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

HOME = Path.home()
CONFIG_PATH = HOME / ".local/share/agent-panel/config.json"
RUNTIME_DIR = HOME / ".local/share/agent-panel"
DEFAULT_ROOTS = [
    HOME / "Developer/company/云仓/WMS",
    HOME / "Developer/tools/agent-panel",
]
LEGACY_ID_RE = re.compile(r"^WMS-(\d+)-(.+)$")
GRP_ID_RE = re.compile(r"^WMS-GRP-(\d+)(?:-|$)")
TEXT_SUFFIXES = {".md", ".json", ".jsonl", ".yaml", ".yml", ".txt"}
SKIP_PARTS = {".git", "worktree", "node_modules", "target", "dist", "tmp"}
GROUP_FILE = "group.json"


def load_scan_roots() -> list[Path]:
    roots: list[Path] = []
    if CONFIG_PATH.exists():
        try:
            cfg = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            roots = [Path(p) for p in cfg.get("requirementScanRoots", [])]
        except (json.JSONDecodeError, OSError):
            roots = []
    if not roots:
        roots = DEFAULT_ROOTS
    return [r for r in roots if r.exists()]


def iter_req_dirs(root: Path) -> list[Path]:
    """Requirement dirs = directories containing meta.md (skip build/vendored
    trees, but allow .agents/ which is where req roots live)."""
    out: set[Path] = set()
    for meta in root.rglob("meta.md"):
        dir_path = meta.parent
        rel_parts = dir_path.relative_to(root).parts
        if any(part in SKIP_PARTS for part in rel_parts):
            continue
        out.add(dir_path)
    return sorted(out)


def is_group_dir(dir_path: Path) -> bool:
    return (dir_path / GROUP_FILE).is_file()


def next_grp_seq(all_req_dirs: list[Path]) -> int:
    max_seq = 0
    for dir_path in all_req_dirs:
        m = GRP_ID_RE.match(dir_path.name)
        if m:
            max_seq = max(max_seq, int(m.group(1)))
    return max_seq + 1


def rewrite_file(file_path: Path, replacements: dict[str, str], execute: bool) -> bool:
    try:
        text = file_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return False
    new_text = text
    for old, new in replacements.items():
        new_text = new_text.replace(old, new)
    if new_text == text:
        return False
    if execute:
        file_path.write_text(new_text, encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="apply changes (default: dry-run)")
    args = parser.parse_args()
    execute = args.execute

    roots = load_scan_roots()
    print(f"scan roots: {[str(r) for r in roots]}")
    all_req_dirs = [d for root in roots for d in iter_req_dirs(root)]
    group_dirs = [d for d in all_req_dirs if is_group_dir(d)]

    renames: list[tuple[Path, str, str]] = []
    for dir_path in group_dirs:
        m = LEGACY_ID_RE.match(dir_path.name)
        if not m:
            continue  # already WMS-GRP-* or non-standard: skip
        seq, rest = m.groups()
        new_id = f"WMS-GRP-{next_grp_seq(all_req_dirs):03d}-{rest}"
        # recompute per rename so multiple renames get distinct seqs
        all_req_dirs.append(dir_path.parent / new_id)
        renames.append((dir_path, dir_path.name, new_id))
    skipped = [d.name for d in group_dirs if not LEGACY_ID_RE.match(d.name)]
    for name in skipped:
        print(f"skip (already WMS-GRP-*/non-standard): {name}")

    if not renames:
        print("no legacy group ids found; nothing to do")
        return 0

    replacements = {old: new for _, old, new in renames}
    print(f"\n{len(renames)} group dir(s) to rename:")
    for dir_path, old_id, new_id in renames:
        target = dir_path.parent / new_id
        status = "EXISTS!" if target.exists() else "ok"
        print(f"  {old_id}  ->  {new_id}  [{status}]")
        if target.exists():
            print("abort: target directory already exists", file=sys.stderr)
            return 1

    rewritten = 0

    # 1) rename dirs
    for dir_path, _old_id, new_id in renames:
        new_path = dir_path.parent / new_id
        print(f"rename: {dir_path.name} -> {new_id}")
        if execute:
            dir_path.rename(new_path)

    # 2) rewrite contents inside renamed group dirs
    renamed_original = {dir_path for dir_path, _, _ in renames}
    for dir_path, old_id, new_id in renames:
        new_path = dir_path.parent / new_id
        # dry-run: dir not renamed yet, scan the original dir; execute: scan the renamed one.
        scan_path = dir_path if not execute else new_path
        for file_path in sorted(p for p in scan_path.rglob("*") if p.is_file()):
            if file_path.suffix not in TEXT_SUFFIXES:
                continue
            if rewrite_file(file_path, {old_id: new_id}, execute):
                print(f"  rewrite: {new_path / file_path.relative_to(scan_path)}")
                rewritten += 1

    # 3) cross references in all other req dirs
    for dir_path in all_req_dirs:
        if dir_path in renamed_original or not dir_path.exists():
            continue
        for file_path in sorted(p for p in dir_path.rglob("*") if p.is_file()):
            if file_path.suffix not in TEXT_SUFFIXES:
                continue
            if rewrite_file(file_path, replacements, execute):
                print(f"  xref: {file_path}")
                rewritten += 1

    # 4) runtime data (~/.local/share/agent-panel/*.json *.jsonl)
    if RUNTIME_DIR.exists():
        for file_path in sorted(RUNTIME_DIR.glob("*.json*")):
            if rewrite_file(file_path, replacements, execute):
                print(f"  runtime: {file_path}")
                rewritten += 1

    mode = "APPLIED" if execute else "DRY-RUN"
    print(f"\n{mode}: {len(renames)} rename(s), {rewritten} file(s) rewritten")
    if not execute:
        print("re-run with --execute to apply")
    return 0


if __name__ == "__main__":
    sys.exit(main())

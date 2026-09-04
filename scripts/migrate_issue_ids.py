#!/usr/bin/env python3
"""Migrate legacy online-issue req ids to the WMS-INC- numbering pool.

One-off (re-runnable) migration for Agent Panel requirement directories:

- Scans `requirementScanRoots` from ~/.local/share/agent-panel/config.json
  (falls back to the two known roots) for requirement dirs whose category is
  线上问题. Category source of truth is state.json (matches the panel);
  meta.md frontmatter is only a fallback.
- Renames dirs `WMS-<seq>-<slug>` -> `WMS-INC-<seq:03d>-<slug>` (keeps the
  original number; dirs already using WMS-INC- are skipped).
- Rewrites the old full id to the new full id inside every text file of the
  renamed dir (meta.md / state.json / notes.md / events.jsonl / ...).
- Aligns the renamed dir's meta.md `category:` line with state.json when they
  disagree (setCategory only writes state.json, meta can go stale).
- Rewrites cross references in ALL other requirement dirs under the scan
  roots (meta.md `issues:` bindings, notes, events).
- Rewrites runtime data under ~/.local/share/agent-panel/ (*.json/*.jsonl
  mentioning the old full id, e.g. associations.json keys).

Dry-run by default; pass --execute to apply. Idempotent: re-running skips
dirs already migrated and replacements that no longer match.
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
TEXT_SUFFIXES = {".md", ".json", ".jsonl", ".yaml", ".yml", ".txt"}
SKIP_PARTS = {".git", "worktree", "node_modules", "target", "dist", "tmp"}
ISSUE_CATEGORY = "线上问题"


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


def read_state_category(dir_path: Path) -> str | None:
    state_path = dir_path / "state.json"
    if not state_path.exists():
        return None
    try:
        return json.loads(state_path.read_text(encoding="utf-8")).get("category")
    except (json.JSONDecodeError, OSError):
        return None


def read_meta_category(meta_path: Path) -> str | None:
    in_frontmatter = False
    for line in meta_path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.strip() == "---":
            if in_frontmatter:
                break
            in_frontmatter = True
            continue
        if in_frontmatter and line.startswith("category:"):
            return line.partition(":")[2].strip().strip('"')
    return None


def dir_category(dir_path: Path) -> str | None:
    """state.json is the category source of truth; meta.md as fallback."""
    return read_state_category(dir_path) or read_meta_category(dir_path / "meta.md")


def plan_rename(dir_path: Path) -> tuple[str, str] | None:
    m = LEGACY_ID_RE.match(dir_path.name)
    if not m:
        return None  # already WMS-INC-* or non-standard: skip
    seq, rest = m.groups()
    return dir_path.name, f"WMS-INC-{int(seq):03d}-{rest}"


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


def align_meta_category(new_path: Path, execute: bool) -> bool:
    """meta.md `category:` line may be stale (setCategory writes state.json only)."""
    state_category = read_state_category(new_path)
    meta_path = new_path / "meta.md"
    if not state_category or not meta_path.exists():
        return False
    try:
        text = meta_path.read_text(encoding="utf-8")
    except OSError:
        return False
    new_text, n = re.subn(
        rf"(?m)^category: *\"?{re.escape(state_category)}\"?$",
        lambda m: m.group(0),
        text,
    )
    if n:
        return False  # already aligned
    new_text, n = re.subn(
        r"(?m)^category:.*$",
        f"category: {state_category}",
        text,
        count=1,
    )
    if not n or new_text == text:
        return False
    if execute:
        meta_path.write_text(new_text, encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="apply changes (default: dry-run)")
    args = parser.parse_args()
    execute = args.execute

    roots = load_scan_roots()
    print(f"scan roots: {[str(r) for r in roots]}")
    all_req_dirs = [d for root in roots for d in iter_req_dirs(root)]
    issue_dirs = [d for d in all_req_dirs if dir_category(d) == ISSUE_CATEGORY]

    renames: list[tuple[Path, str, str]] = []
    skipped_nonstandard: list[str] = []
    for dir_path in issue_dirs:
        planned = plan_rename(dir_path)
        if planned:
            renames.append((dir_path, *planned))
        else:
            skipped_nonstandard.append(dir_path.name)
    for name in skipped_nonstandard:
        print(f"skip (non-standard id, review manually): {name}")

    if not renames:
        print("no legacy issue ids found; nothing to do")
        return 0

    replacements = {old: new for _, old, new in renames}
    print(f"\n{len(renames)} issue dir(s) to rename:")
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

    # 2) rewrite contents inside renamed issue dirs + align meta category
    for dir_path, old_id, new_id in renames:
        new_path = dir_path.parent / new_id
        for file_path in sorted(p for p in new_path.rglob("*") if p.is_file()):
            if file_path.suffix not in TEXT_SUFFIXES:
                continue
            if rewrite_file(file_path, {old_id: new_id}, execute):
                print(f"  rewrite: {file_path}")
                rewritten += 1
        if align_meta_category(new_path, execute):
            print(f"  meta category aligned: {new_path / 'meta.md'}")
            rewritten += 1

    # 3) cross references in all other req dirs (skip the dirs renamed above)
    renamed_original = {dir_path for dir_path, _, _ in renames}
    for dir_path in all_req_dirs:
        if dir_path in renamed_original:
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

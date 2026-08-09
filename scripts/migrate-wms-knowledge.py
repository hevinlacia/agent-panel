#!/usr/bin/env python3
"""Migrate WMS legacy knowledge docs into app-managed items/meta layout.

Input:
  <WMS_ROOT>/.agents/knowledge/wms/*.md

Output:
  <WMS_ROOT>/.agents/business-knowledge/{items,meta,index.jsonl}
  <WMS_ROOT>/.agents/experiences/{items,meta,index.jsonl}

The script is idempotent: it rewrites generated item/meta/index files from the
legacy source files, preserving the source files until the caller chooses to
archive/remove them.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

BUSINESS_PREFIXES = ("api", "biz", "conventions", "link", "profile", "ref")
EXPERIENCE_PREFIXES = ("pitfall",)
SKIP_FILES = {"README.md"}


@dataclass
class LegacyDoc:
    path: Path
    id: str
    title: str
    category: str
    kind: str
    out_root: Path
    item_path: Path
    meta_path: Path
    summary: str
    trigger_terms: list[str]
    tags: list[str]
    created_at: str
    updated_at: str


def rfc3339_from_ts(ts: float) -> str:
    return datetime.fromtimestamp(ts, timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def split_frontmatter(text: str) -> tuple[dict[str, str], str]:
    text = text.replace("\r\n", "\n")
    if not text.startswith("---\n"):
        return {}, text
    lines = text.split("\n")
    try:
        end = lines[1:].index("---") + 1
    except ValueError:
        return {}, text
    fields: dict[str, str] = {}
    for line in lines[1:end]:
        if not line.strip() or line.lstrip().startswith("#") or ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip().strip('"').strip("'")
    return fields, "\n".join(lines[end + 1 :])


def yaml_scalar(value: str) -> str:
    value = value or ""
    if re.fullmatch(r"[A-Za-z0-9_.:/?#%&=+@-]+", value):
        return value
    return json.dumps(value, ensure_ascii=False)


def yaml_list(values: list[str]) -> str:
    clean = [v.strip() for v in values if v.strip()]
    if not clean:
        return "[]"
    return "[" + ", ".join(yaml_scalar(v) for v in clean) + "]"


def first_heading(body: str, fallback: str) -> str:
    for line in body.splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            title = stripped.lstrip("#").strip()
            if title:
                return title
    return fallback


def first_prefixed_line(body: str, prefixes: tuple[str, ...]) -> str | None:
    for line in body.splitlines():
        stripped = line.strip()
        for prefix in prefixes:
            if stripped.startswith(prefix):
                return stripped[len(prefix) :].strip()
    return None


def first_paragraph(body: str) -> str:
    body = body.replace("\r\n", "\n")
    for paragraph in body.strip().split("\n\n"):
        lines = [line.strip() for line in paragraph.splitlines() if line.strip() and not line.strip().startswith("#")]
        if not lines:
            continue
        text = " ".join(lines)
        text = re.sub(r"\s+", " ", text).strip()
        if text:
            return text[:700]
    return ""


def parse_trigger_terms(body: str, title: str, doc_id: str) -> list[str]:
    explicit = first_prefixed_line(body, ("触发词：", "触发词:", "Trigger terms:", "trigger_terms:"))
    if explicit:
        terms = re.split(r"[,，、/;；]\s*", explicit)
    else:
        name_terms = re.split(r"[-_\s]+", doc_id)
        title_terms = re.split(r"[\s/，,、;；：:()（）]+", title)
        terms = [*title_terms, *name_terms]
    seen: set[str] = set()
    out: list[str] = []
    for term in terms:
        term = term.strip().strip("`*[]()（）")
        if len(term) < 2 or term in seen:
            continue
        seen.add(term)
        out.append(term)
    return out[:12]


def classify(path: Path) -> tuple[str, str]:
    stem = path.stem
    prefix = stem.split("-", 1)[0]
    if prefix in EXPERIENCE_PREFIXES:
        return "experience", prefix
    if prefix in BUSINESS_PREFIXES:
        return "businessKnowledge", prefix
    return "businessKnowledge", prefix or "general"


def source_id(kind: str, legacy_stem: str) -> str:
    if kind == "experience" and legacy_stem.startswith("pitfall-"):
        return f"exp-wms-{legacy_stem[len('pitfall-') :]}"
    return legacy_stem


def make_doc(path: Path, wms_root: Path) -> LegacyDoc | None:
    if path.name in SKIP_FILES or path.name.startswith("."):
        return None
    kind, category = classify(path)
    target_base = wms_root / ".agents" / ("experiences" if kind == "experience" else "business-knowledge")
    raw = path.read_text(encoding="utf-8", errors="replace")
    fields, body = split_frontmatter(raw)
    doc_id = fields.get("id") or source_id(kind, path.stem)
    title = fields.get("title") or first_heading(body, path.stem)
    summary = fields.get("summary") or first_prefixed_line(body, ("用于：", "用于:", "Purpose:")) or first_paragraph(body) or title
    trigger_terms = parse_trigger_terms(body, title, path.stem)
    tags = ["wms", category]
    if path.stem.split("-", 1)[-1]:
        tags.extend(re.split(r"[-_]+", path.stem.split("-", 1)[-1])[:5])
    stat = path.stat()
    created_at = fields.get("created_at") or fields.get("createdAt") or rfc3339_from_ts(getattr(stat, "st_ctime", stat.st_mtime))
    updated_at = fields.get("updated_at") or fields.get("updatedAt") or rfc3339_from_ts(stat.st_mtime)
    return LegacyDoc(
        path=path,
        id=doc_id,
        title=title,
        category=category,
        kind=kind,
        out_root=target_base,
        item_path=target_base / "items" / f"{doc_id}.md",
        meta_path=target_base / "meta" / f"{doc_id}.yaml",
        summary=summary,
        trigger_terms=trigger_terms,
        tags=list(dict.fromkeys(tags)),
        created_at=created_at,
        updated_at=updated_at,
    )


def render_meta(doc: LegacyDoc) -> str:
    rel_source = f"../items/{doc.item_path.name}"
    type_name = "experience" if doc.kind == "experience" else "business_knowledge"
    return "\n".join(
        [
            f"id: {yaml_scalar(doc.id)}",
            f"title: {yaml_scalar(doc.title)}",
            f"kind: {yaml_scalar(doc.kind)}",
            f"type: {yaml_scalar(type_name)}",
            f"category: {yaml_scalar(doc.category)}",
            "domain: wms",
            "project: WMS",
            "scope: project",
            "status: active",
            "confidence: medium",
            f"created_at: {yaml_scalar(doc.created_at)}",
            f"updated_at: {yaml_scalar(doc.updated_at)}",
            f"source_path: {yaml_scalar(rel_source)}",
            f"origin_path: {yaml_scalar(str(doc.path))}",
            f"summary: {yaml_scalar(doc.summary)}",
            f"tags: {yaml_list(doc.tags)}",
            f"trigger_terms: {yaml_list(doc.trigger_terms)}",
            "",
        ]
    )


def write_index(root: Path, docs: list[LegacyDoc]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    lines = []
    for doc in sorted(docs, key=lambda d: d.id):
        lines.append(json.dumps({
            "id": doc.id,
            "title": doc.title,
            "kind": doc.kind,
            "category": doc.category,
            "domain": "wms",
            "project": "WMS",
            "scope": "project",
            "status": "active",
            "confidence": "medium",
            "createdAt": doc.created_at,
            "updatedAt": doc.updated_at,
            "summary": doc.summary,
            "triggerTerms": doc.trigger_terms,
            "tags": doc.tags,
            "sourcePath": f"items/{doc.item_path.name}",
            "metaPath": f"meta/{doc.meta_path.name}",
            "originPath": str(doc.path),
        }, ensure_ascii=False))
    (root / "index.jsonl").write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")


def write_readme(root: Path, kind: str, docs: list[LegacyDoc]) -> None:
    root.mkdir(parents=True, exist_ok=True)
    title = "WMS Business Knowledge" if kind == "businessKnowledge" else "WMS Experiences"
    desc = "业务事实、规则、接口、表关系和链路知识" if kind == "businessKnowledge" else "排障经验、踩坑记录和短期特化结论"
    categories = sorted({doc.category for doc in docs})
    body = f"""# {title}\n\n用于：存放 WMS {desc}，由 Agent Panel 管理和检索。\n触发词：WMS、业务知识、经验、接口、链路、排障、踩坑。\n不适用：测试造数和接口模板能力，已迁移到 `/home/hevin/Developer/tools/wms-testdata-recipes`。\n\n## Layout\n\n- `meta/*.yaml`：程序管理属性文件，含概述、时间、标签、触发词和 `source_path`。纯 YAML，不放人类说明正文。\n- `items/*.md`：完整正文，Agent 只有在需要进一步展开时读取。\n- `index.jsonl`：由迁移/应用生成的检索缓存。\n\n## Categories\n\n{chr(10).join(f'- `{c}`' for c in categories)}\n\n## Agent Contract\n\nAgent 默认通过 Agent Panel API 查询摘要：\n\n```http\nPOST /api/agent/knowledge/query\nGET /api/agent/items/summary?id=<id>\nGET /api/agent/items/full?id=<id>\n```\n\n不要再直接依赖旧 `knowledge/wms/*.md` 的文件名前缀做主检索。\n"""
    (root / "README.md").write_text(body, encoding="utf-8")


def migrate(wms_root: Path, dry_run: bool = False) -> dict[str, int]:
    legacy_dir = wms_root / ".agents" / "knowledge" / "wms"
    if not legacy_dir.is_dir():
        raise SystemExit(f"missing legacy dir: {legacy_dir}")
    docs = [doc for path in sorted(legacy_dir.glob("*.md")) if (doc := make_doc(path, wms_root))]
    by_root: dict[Path, list[LegacyDoc]] = {}
    for doc in docs:
        by_root.setdefault(doc.out_root, []).append(doc)
        if not dry_run:
            doc.item_path.parent.mkdir(parents=True, exist_ok=True)
            doc.meta_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(doc.path, doc.item_path)
            doc.meta_path.write_text(render_meta(doc), encoding="utf-8")
    if not dry_run:
        for root, root_docs in by_root.items():
            kind = root_docs[0].kind if root_docs else "businessKnowledge"
            write_index(root, root_docs)
            write_readme(root, kind, root_docs)
    return {
        "total": len(docs),
        "businessKnowledge": sum(1 for d in docs if d.kind == "businessKnowledge"),
        "experience": sum(1 for d in docs if d.kind == "experience"),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("wms_root", nargs="?", default="/home/hevin/Developer/company/WMS")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    result = migrate(Path(args.wms_root).expanduser().resolve(), args.dry_run)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Enrich WMS Agent Panel knowledge meta files with structured related_* fields.

Adds/updates these optional YAML list fields by scanning each item body:
  - related_skills
  - related_repos
  - related_tables
  - related_apis

The script is deterministic and idempotent. It preserves existing meta content as
much as possible, replacing only the managed related_* lines and regenerating
index.jsonl from the enriched meta summaries.
"""
from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

MANAGED_FIELDS = ["related_skills", "related_repos", "related_tables", "related_apis"]
KNOWLEDGE_DIRS = ["business-knowledge", "experiences"]

SKILL_KEYWORDS: list[tuple[str, list[str]]] = [
    ("wms-auth-auto-login", ["登录", "x-token", "x-client", "验证码", "login.mjs", "鉴权"]),
    ("wms-test-api-call", ["curl", "gateway", "网关", "路由", "302", "404", "503", "controller", "接口调用"]),
    ("wms-test-data-creation", ["造数", "测试数据", "状态图", "复核", "波次", "拣货", "checked", "finishcheck", "task_detail"]),
    ("kibana-log-query", ["es 日志", "kibana", "appname", "traceid", "tid", "match_phrase", "logger"]),
    ("mysql-direct-query-write", ["mysql", "show columns", "select ", "insert ", "update ", "delete ", "数据库"]),
    ("code-map-query", ["影响面", "谁消费", "谁调用", "调用链", "表谁在写"]),
    ("rabbitmq-to-rocketmq", ["rabbitmq", "rocketmq", "mq迁移", "mq 迁移", "双发", "topic", "consumer"]),
    ("rabbitmq-rocketmq-plan", ["rabbitmq", "rocketmq", "迁移方案", "灰度方案"]),
    ("rabbitmq-rocketmq-consumer", ["consumer", "listener", "消费", "mqmessage", "process_history"]),
    ("rabbitmq-rocketmq-producer", ["producer", "发送", "延迟消息"]),
    ("rabbitmq-rocketmq-verify", ["mq验证", "消费验证", "topic 搜不到", "消息验证"]),
    ("rabbitmq-rocketmq-unit-test-write", ["topic/tag", "单元测试", "常量类", "找不到符号", "mockmq"]),
    ("dts-operation-log-config", ["dts", "模板日志", "@logstrategy", "@logcondition", "操作日志配置"]),
    ("dts-operation-log-verify", ["dts", "模板日志", "部署版本", "button_code", "操作日志验证"]),
    ("dts-config-change-log-annotate", ["配置变动", "字段注解", "dts kafka", "binlog"]),
    ("coding-logging-annotate", ["日志规范", "mdc", "链路日志", "log.info"]),
    ("apollo-config-query", ["apollo", "配置中心", "namespace"]),
    ("nacos-config-query", ["nacos", "配置中心", "namespace"]),
    ("wms-business-es-query", ["业务 es", "receipt_header_search", "mapping", "业务索引"]),
    ("wms-web-trigger", ["前端", "菜单", "按钮", "web-front", "web-custom-front", "触发入口"]),
    ("wms-business-doc-writer", ["业务文档", "知识文档", "business map", "业务地图"]),
]
KNOWN_SKILLS = {name for name, _ in SKILL_KEYWORDS}

TABLE_STOPWORDS = {
    "select", "update", "insert", "delete", "from", "into", "where", "set", "and", "or", "join", "left",
    "right", "inner", "outer", "table", "show", "columns", "database", "schema", "index", "key", "value",
    "status", "type", "id", "code", "name", "data", "params", "result", "log", "info", "error", "warn",
    "true", "false", "null", "wms", "api", "biz", "link", "profile", "pitfall", "ref", "conventions",
}

KNOWN_TABLE_HINTS = [
    "_header", "_detail", "_history", "_log", "_config", "_sync", "_inventory", "_container", "_plan",
    "_task", "_wave", "_shipment", "_receipt", "_order", "_item", "_location", "_operation", "_pre_package",
]

REPO_RE = re.compile(r"yl-cwh(?:sea)?-wms-[A-Za-z0-9][A-Za-z0-9_-]*")
CN_DEPLOY_RE = re.compile(r"yl-cwh-wms-[A-Za-z0-9][A-Za-z0-9_-]*")
METHOD_API_RE = re.compile(r"\b(GET|POST|PUT|DELETE|PATCH)\s+((?:https?://[^\s`'\")]+)|(?:/[A-Za-z0-9_./{}:-]+))", re.I)
BACKTICK_DOC_RE = re.compile(r"`((?:api|biz|link|profile|ref|conventions|exp|pitfall)-[A-Za-z0-9_.-]+?)(?:\.md)?`")
SKILL_DIRECT_RE = re.compile(r"`([a-z][a-z0-9-]+)`(?:\s*(?:skill|workflow|流程))?", re.I)
BACKTICK_TOKEN_RE = re.compile(r"`([A-Za-z][A-Za-z0-9_]{2,})`")
SQL_TABLE_RE = re.compile(r"\b(?:FROM|JOIN|INTO|UPDATE|TABLE|SHOW\s+COLUMNS\s+FROM)\s+`?([A-Za-z][A-Za-z0-9_]{2,})`?", re.I)


@dataclass
class KnowledgeItem:
    kind_dir: str
    meta_path: Path
    item_path: Path
    fields: dict[str, str]
    body: str

    @property
    def id(self) -> str:
        return self.fields.get("id") or self.meta_path.stem


def unique_sorted(values: Iterable[str]) -> list[str]:
    seen: set[str] = set()
    out: list[str] = []
    for raw in values:
        value = raw.strip().strip("`'\"，,；;。.)]")
        if not value:
            continue
        if value.lower() in seen:
            continue
        seen.add(value.lower())
        out.append(value)
    return sorted(out, key=lambda s: s.lower())


def yaml_scalar(value: str) -> str:
    if re.fullmatch(r"[A-Za-z0-9_.:/?#%&=+@{}-]+", value):
        return value
    return json.dumps(value, ensure_ascii=False)


def yaml_list(values: list[str]) -> str:
    if not values:
        return "[]"
    return "[" + ", ".join(yaml_scalar(v) for v in values) + "]"


def parse_yamlish(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#") or ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip()
    return fields


def unquote_yaml(value: str) -> str:
    value = value.strip()
    if not value:
        return ""
    if (value.startswith('"') and value.endswith('"')) or (value.startswith("'") and value.endswith("'")):
        try:
            return json.loads(value) if value.startswith('"') else value[1:-1]
        except Exception:
            return value[1:-1]
    return value


def parse_yaml_list(value: str) -> list[str]:
    value = value.strip()
    if not value or value == "[]":
        return []
    if value.startswith("[") and value.endswith("]"):
        body = value[1:-1].strip()
        if not body:
            return []
        try:
            parsed = json.loads("[" + body + "]")
            return [str(v) for v in parsed if str(v).strip()]
        except Exception:
            return [unquote_yaml(part.strip()) for part in re.split(r",\s*", body) if part.strip()]
    return [unquote_yaml(value)]


def resolve_item_path(meta_path: Path, source_path: str) -> Path:
    raw = unquote_yaml(source_path)
    if not raw:
        return meta_path.parent.parent / "items" / f"{meta_path.stem}.md"
    p = Path(raw)
    if p.is_absolute():
        return p
    return (meta_path.parent / p).resolve()


def load_items(wms_root: Path) -> list[KnowledgeItem]:
    items: list[KnowledgeItem] = []
    for kind_dir in KNOWLEDGE_DIRS:
        meta_dir = wms_root / ".agents" / kind_dir / "meta"
        if not meta_dir.is_dir():
            continue
        for meta_path in sorted(meta_dir.glob("*.y*ml")):
            meta_text = meta_path.read_text(encoding="utf-8", errors="replace")
            fields = parse_yamlish(meta_text)
            item_path = resolve_item_path(meta_path, fields.get("source_path", ""))
            body = item_path.read_text(encoding="utf-8", errors="replace") if item_path.exists() else ""
            items.append(KnowledgeItem(kind_dir, meta_path, item_path, fields, body))
    return items


def extract_repos(text: str, fields: dict[str, str]) -> list[str]:
    values = [m.group(0) for m in REPO_RE.finditer(text)]
    values += [m.group(0) for m in CN_DEPLOY_RE.finditer(text)]
    origin = unquote_yaml(fields.get("origin_path", ""))
    values += [m.group(0) for m in REPO_RE.finditer(origin)]
    # Profile docs often carry the app name in title/id/path; keep explicit prefixed values only to avoid over-guessing.
    return unique_sorted(values)


def looks_like_table(token: str, context: str = "") -> bool:
    t = token.strip().lower()
    if not re.fullmatch(r"[a-z][a-z0-9_]{3,}", t):
        return False
    if t in TABLE_STOPWORDS:
        return False
    # Keep this intentionally conservative: WMS real tables are overwhelmingly snake_case.
    if "_" not in t:
        return False
    if t.startswith(("wms_", "mq_")) and not any(h in t for h in KNOWN_TABLE_HINTS):
        return False
    if any(h in t for h in KNOWN_TABLE_HINTS):
        return True
    ctx = context.lower()
    return any(word in ctx for word in ["select", "insert", "update", "delete", "from", "join", "相关表", "数据对象", "表名", "columns"])


def extract_tables(text: str) -> list[str]:
    values: list[str] = []
    for m in SQL_TABLE_RE.finditer(text):
        token = m.group(1)
        if looks_like_table(token, text[max(0, m.start() - 80):m.end() + 80]):
            values.append(token.lower())
    for m in BACKTICK_TOKEN_RE.finditer(text):
        token = m.group(1)
        context = text[max(0, m.start() - 80):m.end() + 80]
        if looks_like_table(token, context):
            values.append(token.lower())
    return unique_sorted(values)


def extract_apis(text: str) -> list[str]:
    values: list[str] = []
    for m in METHOD_API_RE.finditer(text):
        method = m.group(1).upper()
        path = m.group(2).rstrip(".,;，。；")
        values.append(f"{method} {path}")
    for m in BACKTICK_DOC_RE.finditer(text):
        doc_id = m.group(1)
        if doc_id.endswith(".md"):
            doc_id = doc_id[:-3]
        values.append(doc_id)
    return unique_sorted(values)


def extract_skills(text: str) -> list[str]:
    lower = text.lower()
    values: list[str] = []
    for m in SKILL_DIRECT_RE.finditer(text):
        value = m.group(1).lower()
        if value in KNOWN_SKILLS:
            values.append(value)
    for skill, keywords in SKILL_KEYWORDS:
        if any(keyword.lower() in lower for keyword in keywords):
            values.append(skill)
    return unique_sorted(values)


def enrich(item: KnowledgeItem) -> dict[str, list[str]]:
    meta_blob = "\n".join(
        f"{k}: {v}" for k, v in item.fields.items() if k not in MANAGED_FIELDS
    )
    text = f"{meta_blob}\n\n{item.body}"
    existing_skills = parse_yaml_list(item.fields.get("related_skills", ""))
    existing_repos = parse_yaml_list(item.fields.get("related_repos", ""))
    existing_tables = parse_yaml_list(item.fields.get("related_tables", ""))
    existing_apis = parse_yaml_list(item.fields.get("related_apis", ""))
    return {
        "related_skills": unique_sorted(existing_skills + extract_skills(text)),
        "related_repos": unique_sorted(existing_repos + extract_repos(text, item.fields)),
        "related_tables": unique_sorted(existing_tables + extract_tables(text)),
        "related_apis": unique_sorted(existing_apis + extract_apis(text)),
    }


def update_meta_text(text: str, relations: dict[str, list[str]]) -> str:
    lines = text.rstrip("\n").splitlines()
    cleaned = [line for line in lines if line.split(":", 1)[0].strip() not in MANAGED_FIELDS]
    insert_after = -1
    for idx, line in enumerate(cleaned):
        key = line.split(":", 1)[0].strip() if ":" in line else ""
        if key in {"related_skills", "trigger_terms", "tags", "summary"}:
            insert_after = idx
    insert = [f"{field}: {yaml_list(relations[field])}" for field in MANAGED_FIELDS if relations[field]]
    if not insert:
        return "\n".join(cleaned).rstrip() + "\n"
    pos = insert_after + 1 if insert_after >= 0 else len(cleaned)
    next_lines = cleaned[:pos] + insert + cleaned[pos:]
    return "\n".join(next_lines).rstrip() + "\n"


def first_paragraph(body: str) -> str:
    for paragraph in body.strip().split("\n\n"):
        lines = [line.strip() for line in paragraph.splitlines() if line.strip() and not line.strip().startswith("#")]
        if lines:
            return re.sub(r"\s+", " ", " ".join(lines)).strip()[:700]
    return ""


def index_record(item: KnowledgeItem, relations: dict[str, list[str]]) -> dict[str, object]:
    body = item.body
    return {
        "id": unquote_yaml(item.fields.get("id", item.meta_path.stem)),
        "title": unquote_yaml(item.fields.get("title", item.meta_path.stem)),
        "kind": unquote_yaml(item.fields.get("kind", "experience" if item.kind_dir == "experiences" else "businessKnowledge")),
        "category": unquote_yaml(item.fields.get("category", "")),
        "domain": unquote_yaml(item.fields.get("domain", "wms")),
        "project": unquote_yaml(item.fields.get("project", "WMS")),
        "scope": unquote_yaml(item.fields.get("scope", "project")),
        "status": unquote_yaml(item.fields.get("status", "active")),
        "confidence": unquote_yaml(item.fields.get("confidence", "medium")),
        "summary": unquote_yaml(item.fields.get("summary", "")) or first_paragraph(body),
        "tags": parse_yaml_list(item.fields.get("tags", "")),
        "triggerTerms": parse_yaml_list(item.fields.get("trigger_terms", "")),
        "relatedSkills": relations["related_skills"],
        "relatedRepos": relations["related_repos"],
        "relatedTables": relations["related_tables"],
        "relatedApis": relations["related_apis"],
        "sourcePath": str(item.item_path),
        "metaPath": str(item.meta_path),
        "originPath": unquote_yaml(item.fields.get("origin_path", "")),
        "updatedAt": unquote_yaml(item.fields.get("updated_at", "")),
    }


def write_indexes(wms_root: Path, items: list[KnowledgeItem], rel_by_id: dict[Path, dict[str, list[str]]]) -> None:
    for kind_dir in KNOWLEDGE_DIRS:
        root = wms_root / ".agents" / kind_dir
        rows = [index_record(item, rel_by_id[item.meta_path]) for item in items if item.kind_dir == kind_dir]
        rows.sort(key=lambda row: str(row.get("id", "")))
        (root / "index.jsonl").write_text(
            "".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows),
            encoding="utf-8",
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wms-root", default="/home/hevin/Developer/company/WMS")
    parser.add_argument("--write", action="store_true", help="Write changes. Default is dry-run.")
    parser.add_argument("--limit", type=int, default=0, help="Print first N item details.")
    args = parser.parse_args()

    wms_root = Path(args.wms_root).expanduser().resolve()
    items = load_items(wms_root)
    rel_by_path: dict[Path, dict[str, list[str]]] = {}
    changed: list[tuple[KnowledgeItem, dict[str, list[str]], dict[str, list[str]]]] = []
    for item in items:
        relations = enrich(item)
        rel_by_path[item.meta_path] = relations
        before = {field: parse_yaml_list(item.fields.get(field, "")) for field in MANAGED_FIELDS}
        if before != relations:
            changed.append((item, before, relations))

    print(f"items={len(items)} changed={len(changed)} mode={'write' if args.write else 'dry-run'}")
    counts = {field: sum(1 for item in items if rel_by_path[item.meta_path][field]) for field in MANAGED_FIELDS}
    print("populated=" + json.dumps(counts, ensure_ascii=False, sort_keys=True))
    for item, before, after in changed[: args.limit or 10]:
        print(f"\n- {item.id} ({item.kind_dir})")
        for field in MANAGED_FIELDS:
            if before[field] != after[field]:
                print(f"  {field}: {after[field][:12]}{' ...' if len(after[field]) > 12 else ''}")

    if args.write:
        for item in items:
            text = item.meta_path.read_text(encoding="utf-8", errors="replace")
            next_text = update_meta_text(text, rel_by_path[item.meta_path])
            if next_text != text:
                item.meta_path.write_text(next_text, encoding="utf-8")
        # Reload item meta after writes so index reflects exact updated values.
        items = load_items(wms_root)
        rel_by_path = {item.meta_path: enrich(item) for item in items}
        write_indexes(wms_root, items, rel_by_path)
        print("wrote meta files and regenerated index.jsonl")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

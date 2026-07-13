#!/usr/bin/env python3
"""
Observe production logs after requirement deployment.

Reads requirement files to extract log keywords, then queries CN/SEA PRO
Kibana bsearch to verify code execution and check for errors.

Usage:
  # Query specific requirements after deployment
  uv run python observe_req_logs.py --req-ids WMS-003,WMS-009 \
    --start-time "2026-06-30 21:09:00"

  # Auto-discover "待上线" requirements
  uv run python observe_req_logs.py --status 待上线 \
    --start-time "2026-06-30 21:09:00"

  # Only SEA, with extra keywords
  uv run python observe_req_logs.py --req-ids WMS-003 --envs sea \
    --start-time "2026-06-30 21:09:00" --keywords "extra_keyword1,extra_keyword2"
"""

import argparse
import json
import os
import re
import sys
import time
import uuid
from datetime import datetime, timezone, timedelta
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError

# Import shared auth module
BSEARCH_SCRIPTS_DIR = os.path.expanduser(
    "~/Developer/company/WMS/.agents/skills/wms-kibana-bsearch-payload-query/scripts"
)
sys.path.insert(0, BSEARCH_SCRIPTS_DIR)
import kibana_auth

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

CN_BASE = "http://cwhcn-logskb.jms.com"
SEA_BASE = "http://cwhsea-logskb.jms.com"
BSEARCH_PATH = "/internal/bsearch?compress=false"
KBN_VERSION = "7.16.2"

CN_INDEX = "pro-cwh*-applog*"
SEA_INDEX = "pro-cwhsea*applog*"

REQ_BASE_DIR = os.path.expanduser(
    "~/Developer/infra/ai-code-config/projects/wms/agents/req/WMS"
)

OUT_DIR = "/tmp/opencode/req-log-observe"

# Keywords for error checking
# "Exception" is reliable; "ERROR" matches too many normal logs with "error":false
ERROR_KEYWORDS = ["Exception"]

# ---------------------------------------------------------------------------
# Time helpers
# ---------------------------------------------------------------------------


def bj_to_utc(s):
    """Convert Beijing time string to UTC ISO format."""
    dt = datetime.strptime(s, "%Y-%m-%d %H:%M:%S")
    dt = dt.replace(tzinfo=timezone(timedelta(hours=8)))
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def now_bj():
    return datetime.now(timezone(timedelta(hours=8))).strftime("%Y-%m-%d %H:%M:%S")


# ---------------------------------------------------------------------------
# Requirement discovery
# ---------------------------------------------------------------------------


def find_requirement_dirs(req_ids=None, status=None):
    """Find requirement directories matching req-ids or status."""
    base = Path(REQ_BASE_DIR)
    if not base.exists():
        print(f"ERROR: Requirement base dir not found: {base}", file=sys.stderr)
        sys.exit(1)

    all_dirs = sorted([d for d in base.iterdir() if d.is_dir()])

    if req_ids:
        ids = [r.strip() for r in req_ids.split(",")]
        matched = []
        for d in all_dirs:
            for rid in ids:
                if d.name.startswith(rid):
                    matched.append(d)
                    break
        return matched

    if status:
        matched = []
        for d in all_dirs:
            state_file = d / "state.json"
            if state_file.exists():
                try:
                    state = json.loads(state_file.read_text())
                    if state.get("status") == status:
                        matched.append(d)
                except (json.JSONDecodeError, KeyError):
                    pass
            # Also check meta.md frontmatter
            meta_file = d / "meta.md"
            if meta_file.exists():
                content = meta_file.read_text()
                m = re.search(r"^status:\s*(.+)$", content, re.MULTILINE)
                if m and m.group(1).strip() == status:
                    if d not in matched:
                        matched.append(d)
        return matched

    return all_dirs


# ---------------------------------------------------------------------------
# Keyword extraction
# ---------------------------------------------------------------------------


def extract_keywords(req_dir):
    """Extract log observation keywords from requirement files.

    Extraction priority:
    1. `## 日志观测关键字` section in test.md or notes.md
    2. `Kibana 搜索关键字` pattern in test.md
    3. `flag:"..."` patterns in notes.md
    4. Config switch names like `mq.switch.xxx`
    """
    keywords = set()

    # Read test.md and notes.md
    test_md = (
        (req_dir / "test.md").read_text() if (req_dir / "test.md").exists() else ""
    )
    notes_md = (
        (req_dir / "notes.md").read_text() if (req_dir / "notes.md").exists() else ""
    )
    meta_md = (
        (req_dir / "meta.md").read_text() if (req_dir / "meta.md").exists() else ""
    )

    for content in [test_md, notes_md]:
        # Pattern 1: ## 日志观测关键字 section
        section_match = re.search(
            r"##\s*日志观测关键字\s*\n(.*?)(?=\n##\s|\Z)",
            content,
            re.DOTALL,
        )
        if section_match:
            section_text = section_match.group(1)
            # Extract backtick-quoted strings
            for m in re.finditer(r"`([^`]+)`", section_text):
                kw = m.group(1).strip()
                if kw and len(kw) > 2:
                    keywords.add(kw)
            # Also extract non-backtick list items
            for line in section_text.strip().split("\n"):
                line = line.strip()
                if line.startswith("- ") or line.startswith("* "):
                    item = line[2:].strip().strip("`").strip('"').strip("'")
                    if item and len(item) > 2 and not item.startswith("|"):
                        keywords.add(item)

        # Pattern 2: Kibana 搜索关键字
        kibana_matches = re.findall(
            r"Kibana\s*搜索关键字[：:]\s*(.+)",
            content,
        )
        for match in kibana_matches:
            # Extract quoted strings
            for m in re.finditer(r'[""\'`]([^""\'`]+)[""\'`]', match):
                kw = m.group(1).strip()
                if kw and len(kw) > 2:
                    keywords.add(kw)

    # Pattern 3: flag:"..." in notes.md
    for m in re.finditer(r'flag:"([^"]+)"', notes_md):
        kw = m.group(1).strip()
        if kw and len(kw) > 2:
            keywords.add(kw)

    # Pattern 4: Config switch names (informational, not primary keywords)
    # These are less useful as log keywords, skip unless explicitly in keyword section

    # Extract requirement title from meta.md
    title = ""
    m = re.search(r"^title:\s*(.+)$", meta_md, re.MULTILINE)
    if m:
        title = m.group(1).strip()

    # Extract requirement ID from meta.md
    req_id = ""
    m = re.search(r"^req-id:\s*(.+)$", meta_md, re.MULTILINE)
    if m:
        req_id = m.group(1).strip()
    else:
        req_id = req_dir.name

    return {
        "req_id": req_id,
        "title": title,
        "keywords": sorted(keywords),
    }


# ---------------------------------------------------------------------------
# bsearch query
# ---------------------------------------------------------------------------


def build_bsearch_body(index, keyword, start_utc, end_utc, size=50):
    """Build a bsearch request body for keyword search."""
    return {
        "batch": [
            {
                "request": {
                    "params": {
                        "index": index,
                        "body": {
                            "sort": [
                                {
                                    "@timestamp": {
                                        "order": "desc",
                                        "unmapped_type": "boolean",
                                    }
                                }
                            ],
                            "size": size,
                            "version": True,
                            "_source": True,
                            "query": {
                                "bool": {
                                    "must": [],
                                    "filter": [
                                        {
                                            "multi_match": {
                                                "type": "phrase",
                                                "query": keyword,
                                                "lenient": True,
                                            }
                                        },
                                        {
                                            "range": {
                                                "@timestamp": {
                                                    "format": "strict_date_optional_time",
                                                    "gte": start_utc,
                                                    "lte": end_utc,
                                                }
                                            }
                                        },
                                    ],
                                    "should": [],
                                    "must_not": [],
                                }
                            },
                        },
                        "track_total_hits": True,
                        "preference": int(time.time() * 1000),
                    }
                },
                "options": {
                    "sessionId": str(uuid.uuid4()),
                    "isRestore": False,
                    "strategy": "ese",
                    "isStored": False,
                    "executionContext": {
                        "type": "application",
                        "name": "discover",
                        "description": "fetch documents",
                        "url": "",
                        "id": "",
                    },
                },
            }
        ]
    }


def extract_hits_from_response(resp_data):
    """Extract hits from various bsearch response shapes."""
    if not isinstance(resp_data, dict):
        return [], 0

    # Shape 1: {id, result} — CN style, result has rawResponse
    result = resp_data.get("result")
    if isinstance(result, dict):
        raw_resp = result.get("rawResponse")
        if raw_resp is None and "body" in result:
            raw_resp = result.get("body")
        if isinstance(raw_resp, str):
            try:
                raw_resp = json.loads(raw_resp)
            except json.JSONDecodeError:
                pass
        if isinstance(raw_resp, dict):
            hits_container = raw_resp.get("hits", {})
        elif isinstance(raw_resp, dict) and "hits" in raw_resp:
            hits_container = raw_resp.get("hits", {})
        elif isinstance(result, dict) and "hits" in result:
            hits_container = result.get("hits", {})
        else:
            return [], 0
    elif "hits" in resp_data:
        hits_container = resp_data.get("hits", {})
    else:
        # Try batch shape
        batch = resp_data.get("batch", [])
        if batch and isinstance(batch[0], dict):
            first = batch[0]
            result = first.get("result", {})
            raw_resp = result.get("rawResponse") if isinstance(result, dict) else None
            if raw_resp is None and isinstance(result, dict) and "body" in result:
                raw_resp = result.get("body")
            if isinstance(raw_resp, str):
                try:
                    raw_resp = json.loads(raw_resp)
                except json.JSONDecodeError:
                    pass
            if isinstance(raw_resp, dict):
                hits_container = raw_resp.get("hits", {})
            else:
                return [], 0
        else:
            return [], 0

    total_raw = hits_container.get("total", 0)
    if isinstance(total_raw, dict):
        total = total_raw.get("value", 0)
    else:
        total = total_raw

    hits = hits_container.get("hits", [])
    return hits, total


def result_is_running(resp_data):
    """Check if bsearch response is still running (async)."""
    if not isinstance(resp_data, dict):
        return False
    result = resp_data.get("result")
    if isinstance(result, dict):
        return bool(result.get("isRunning"))
    batch = resp_data.get("batch", [])
    if batch and isinstance(batch[0], dict):
        result = batch[0].get("result")
        if isinstance(result, dict):
            return bool(result.get("isRunning"))
    return False


def result_async_id(resp_data):
    """Extract async result id from bsearch response."""
    if not isinstance(resp_data, dict):
        return ""
    # CN style: id at top level
    top_id = resp_data.get("id")
    if top_id:
        return top_id
    result = resp_data.get("result")
    if isinstance(result, dict) and result.get("id"):
        return result.get("id")
    batch = resp_data.get("batch", [])
    if batch and isinstance(batch[0], dict):
        result = batch[0].get("result")
        if isinstance(result, dict):
            return result.get("id") or ""
    return ""


def query_keyword(region, base_url, index, sid, keyword, start_utc, end_utc, size=50):
    """Query one keyword in one region. Returns (hits, total, error)."""
    endpoint = f"{base_url}{BSEARCH_PATH}"
    headers = {
        "Content-Type": "application/json",
        "kbn-version": KBN_VERSION,
        "Origin": base_url,
        "Referer": f"{base_url}/app/discover",
        "Cookie": f"sid={sid}",
    }

    body = json.dumps(
        build_bsearch_body(index, keyword, start_utc, end_utc, size)
    ).encode("utf-8")
    req = Request(endpoint, data=body, headers=headers, method="POST")

    try:
        with urlopen(req, timeout=60) as resp:
            raw_resp = resp.read()
    except HTTPError as e:
        if kibana_auth.is_auth_error(e):
            return None, 0, f"SID expired (HTTP {e.code})"
        return None, 0, f"HTTP {e.code} {e.reason}"
    except URLError as e:
        return None, 0, f"URL error: {e.reason}"
    except Exception as e:
        return None, 0, f"Error: {e}"

    try:
        resp_data = json.loads(raw_resp)
    except json.JSONDecodeError:
        return None, 0, "JSON decode error"

    # Handle async polling (CN bsearch can be slow for large result sets)
    poll_count = 0
    max_polls = 60
    while (
        result_is_running(resp_data)
        and result_async_id(resp_data)
        and poll_count < max_polls
    ):
        poll_count += 1
        time.sleep(2)
        poll_body = {
            "batch": [
                {
                    "request": {"id": result_async_id(resp_data)},
                    "options": {"strategy": "ese"},
                }
            ]
        }
        poll_req = Request(
            endpoint,
            data=json.dumps(poll_body).encode("utf-8"),
            headers=headers,
            method="POST",
        )
        try:
            with urlopen(poll_req, timeout=60) as resp:
                resp_data = json.loads(resp.read())
        except Exception as e:
            return None, 0, f"Poll error: {e}"

    if result_is_running(resp_data):
        return None, 0, f"Timeout after {max_polls} polls"

    hits, total = extract_hits_from_response(resp_data)

    # Extract key fields from hits
    records = []
    for h in hits:
        src = h.get("_source", {}) or {}
        ts = src.get("@timestamp", "")
        app = src.get("appName", "")
        logmsg = src.get("logmsg", src.get("message", ""))
        records.append({"timestamp": ts, "appName": app, "logmsg": str(logmsg)[:500]})

    return records, total, None


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def parse_args():
    parser = argparse.ArgumentParser(
        description="Observe PRO logs after requirement deployment"
    )
    parser.add_argument(
        "--req-ids", default=None, help="Comma-separated requirement IDs (prefix match)"
    )
    parser.add_argument("--status", default=None, help="Filter by status (e.g. 待上线)")
    parser.add_argument(
        "--start-time", required=True, help="Start time BJT (YYYY-MM-DD HH:MM:SS)"
    )
    parser.add_argument("--end-time", default=None, help="End time BJT (default: now)")
    parser.add_argument(
        "--envs",
        choices=["cn", "sea", "all"],
        default="all",
        help="Environments to query",
    )
    parser.add_argument(
        "--keywords", default=None, help="Extra keywords, comma-separated"
    )
    parser.add_argument(
        "--no-error-check", action="store_true", help="Skip ERROR/Exception check"
    )
    parser.add_argument(
        "--size", type=int, default=50, help="Max results per keyword (default: 50)"
    )
    return parser.parse_args()


def main():
    args = parse_args()

    end_time = args.end_time or now_bj()
    start_utc = bj_to_utc(args.start_time)
    end_utc = bj_to_utc(end_time)

    print(f"Time window: {args.start_time} ~ {end_time} BJT")
    print(f"UTC: {start_utc} ~ {end_utc}")

    # Discover requirements
    req_dirs = find_requirement_dirs(req_ids=args.req_ids, status=args.status)
    if not req_dirs:
        print("No matching requirements found.", file=sys.stderr)
        sys.exit(1)

    print(f"\nFound {len(req_dirs)} requirement(s):")
    for d in req_dirs:
        print(f"  - {d.name}")

    # Extract keywords from each requirement
    req_data = []
    for d in req_dirs:
        info = extract_keywords(d)
        req_data.append(info)
        print(f"\n[{info['req_id']}] {info['title']}")
        print(f"  Keywords: {info['keywords'] if info['keywords'] else '(none)'}")

    # Add extra keywords
    extra_kws = []
    if args.keywords:
        extra_kws = [k.strip() for k in args.keywords.split(",") if k.strip()]

    # Collect all unique keywords
    all_keywords = set()
    for info in req_data:
        all_keywords.update(info["keywords"])
    all_keywords.update(extra_kws)

    # Add error keywords
    check_errors = not args.no_error_check
    if check_errors:
        all_keywords.update(ERROR_KEYWORDS)

    if not all_keywords:
        print(
            "\nNo keywords found. Use --keywords to specify manually.", file=sys.stderr
        )
        sys.exit(1)

    print(f"\n{'=' * 60}")
    print(f"Total unique keywords: {len(all_keywords)}")
    for kw in sorted(all_keywords):
        print(f"  - {kw}")
    print(f"{'=' * 60}")

    # Resolve SIDs
    envs_to_query = ["cn", "sea"] if args.envs == "all" else [args.envs]
    sids = {}

    for env in envs_to_query:
        base_url = CN_BASE if env == "cn" else SEA_BASE
        env_prefix = "CN" if env == "cn" else "SEA"
        try:
            sid = kibana_auth.resolve_sid(
                region=env,
                base_url=base_url,
                cli_sid=None,
                env_sid_var=f"OPENCODE_KIBANA_{env_prefix}_SID",
                env_user_var=f"OPENCODE_KIBANA_{env_prefix}_USERNAME",
                env_pass_var=f"OPENCODE_KIBANA_{env_prefix}_PASSWORD",
                allow_auto_login=True,
            )
            sids[env] = sid
            print(f"[{env.upper()}] SID resolved (len={len(sid)})")
        except RuntimeError as e:
            print(f"[{env.upper()}] Failed to resolve SID: {e}", file=sys.stderr)

    # Query each keyword in each environment
    results = {}
    for env in envs_to_query:
        if env not in sids:
            results[env] = {"error": "No SID"}
            continue

        base_url = CN_BASE if env == "cn" else SEA_BASE
        index = CN_INDEX if env == "cn" else SEA_INDEX
        sid = sids[env]

        results[env] = {}
        for kw in sorted(all_keywords):
            print(f"\n[{env.upper()}] Querying: {kw}")
            records, total, error = query_keyword(
                env, base_url, index, sid, kw, start_utc, end_utc, args.size
            )
            if error:
                print(f"  ERROR: {error}")
                results[env][kw] = {"error": error, "total": 0, "records": []}
            else:
                print(f"  Total: {total}, Fetched: {len(records)}")
                for i, r in enumerate(records[:5]):
                    print(
                        f"    [{i + 1}] {r['timestamp']} | {r['appName']} | {r['logmsg'][:200]}"
                    )
                if len(records) > 5:
                    print(f"    ... and {len(records) - 5} more")
                results[env][kw] = {"total": total, "records": records}

    # Build report
    report = {
        "query_time": now_bj(),
        "time_window": {"start": args.start_time, "end": end_time},
        "requirements": [
            {"req_id": r["req_id"], "title": r["title"], "keywords": r["keywords"]}
            for r in req_data
        ],
        "extra_keywords": extra_kws,
        "environments": envs_to_query,
        "results": results,
    }

    # Save report
    os.makedirs(OUT_DIR, exist_ok=True)
    report_path = Path(OUT_DIR) / "report.json"
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)
    print(f"\n{'=' * 60}")
    print(f"Report saved to {report_path}")

    # Print summary
    print(f"\n{'=' * 60}")
    print("SUMMARY")
    print(f"{'=' * 60}")

    for info in req_data:
        req_id = info["req_id"]
        print(f"\n[{req_id}] {info['title']}")
        for kw in info["keywords"]:
            for env in envs_to_query:
                if env in results and kw in results[env]:
                    r = results[env][kw]
                    total = r.get("total", 0)
                    error = r.get("error")
                    status = f"ERROR: {error}" if error else f"{total} hits"
                    print(f"  [{env.upper()}] '{kw}': {status}")

    if check_errors:
        print(f"\n--- Error Check ---")
        for env in envs_to_query:
            if env in results:
                for ekw in ERROR_KEYWORDS:
                    if ekw in results[env]:
                        r = results[env][ekw]
                        total = r.get("total", 0)
                        error = r.get("error")
                        status = f"ERROR: {error}" if error else f"{total} hits"
                        print(f"  [{env.upper()}] '{ekw}': {status}")

    # Print error details if any
    if check_errors:
        for env in envs_to_query:
            if env not in results:
                continue
            for ekw in ERROR_KEYWORDS:
                if ekw not in results[env]:
                    continue
                records = results[env][ekw].get("records", [])
                if records:
                    # Group by appName
                    from collections import Counter

                    apps = Counter(r.get("appName", "") for r in records)
                    print(f"\n[{env.upper()}] {ekw} by app:")
                    for app, cnt in apps.most_common():
                        print(f"  {app}: {cnt}")

    print(f"\nDone.")


if __name__ == "__main__":
    main()

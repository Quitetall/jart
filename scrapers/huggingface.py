#!/usr/bin/env python3
"""HF papers adapter. Single-shot stdio contract (spec §4.0): read one JSON
request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  paper_search {query, limit, topic} -> {"records": [...]}
"""
import json
import sys
from datetime import datetime, timezone
from urllib.parse import quote
from urllib.request import urlopen, Request

HF_SEARCH = "https://huggingface.co/api/papers/search?q={}"


def _ts_and_label(published_at):
    if not published_at:
        return 0, ""
    try:
        iso = published_at[:-1] + "+00:00" if published_at.endswith("Z") else published_at
        dt = datetime.fromisoformat(iso).astimezone(timezone.utc)
        return int(dt.timestamp() * 1000), dt.strftime("%Y-%m-%d")
    except (ValueError, AttributeError):
        return 0, ""


def normalize(raw, topic=""):
    """Pure: HF search JSON (list of {paper:{...}}) -> list of normalized records."""
    out = []
    for item in raw or []:
        p = (item or {}).get("paper") or {}
        pid = p.get("id", "")
        ts, label = _ts_and_label(p.get("publishedAt"))
        summary = p.get("ai_summary") or p.get("summary") or ""
        out.append({
            "kind": "paper",
            "source": "HF",
            "topic": topic,
            "title": p.get("title", "") or "",
            "link": f"https://huggingface.co/papers/{pid}" if pid else "",
            "date_label": label,
            "ts": ts,
            "summary": summary,
            "grounding": summary,
        })
    return out


def _fetch(query):
    req = Request(HF_SEARCH.format(quote(query)), headers={"User-Agent": "research-tool/0.1"})
    with urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op == "paper_search":
        raw = _fetch(args.get("query", ""))
        recs = normalize(raw, topic=args.get("topic", ""))[: int(args.get("limit", 12))]
        return {"records": recs}
    return {"error": f"unknown op: {op}"}


def main():
    data = sys.stdin.read()
    try:
        result = handle(json.loads(data) if data.strip() else {})
    except Exception as e:  # any failure -> structured error, exit 0
        result = {"error": f"{type(e).__name__}: {e}"}
    sys.stdout.write(json.dumps(result))
    sys.stdout.flush()


if __name__ == "__main__":
    main()

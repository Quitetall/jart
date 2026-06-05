#!/usr/bin/env python3
"""Semantic Scholar adapter. Single-shot stdio contract (spec §4.0): read one
JSON request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  search {query, limit, topic} -> {"records": [...]} | {"error": "..."}

Semantic Scholar throttles HARD without S2_API_KEY, returning a {message, code}
envelope instead of {data: [...]}. On non-200 or a missing 'data' key we return
{"error": "..."} so the host degrades gracefully (SourceError, not a crash).
"""
import json
import os
import sys
from datetime import datetime, timezone
from urllib.parse import quote
from urllib.request import urlopen, Request
from urllib.error import HTTPError, URLError

S2_SEARCH = (
    "https://api.semanticscholar.org/graph/v1/paper/search"
    "?query={}&limit={}"
    "&fields=title,abstract,year,venue,url,externalIds,publicationDate"
)


def _ts_and_label(publication_date, year):
    """Prefer publicationDate (YYYY-MM-DD), fall back to year-only."""
    if publication_date:
        try:
            dt = datetime.fromisoformat(publication_date).replace(tzinfo=timezone.utc)
            return int(dt.timestamp() * 1000), publication_date
        except (ValueError, TypeError):
            pass
    if year:
        try:
            dt = datetime(int(year), 1, 1, tzinfo=timezone.utc)
            return int(dt.timestamp() * 1000), str(year)
        except (ValueError, TypeError):
            pass
    return 0, ""


def normalize(data, topic=""):
    """Pure: S2 graph search body -> list of normalized records.

    Accepts either the full envelope ({"data": [...]} or the throttle
    {"message", "code"} envelope) or a bare list. An error/empty envelope
    yields [] so the host turns it into a SourceError, not a crash.
    """
    items = data.get("data") if isinstance(data, dict) else (data or [])
    out = []
    for item in items or []:
        item = item or {}
        abstract = item.get("abstract") or ""
        summary = (abstract[:180] + "…") if abstract else ""
        ts, label = _ts_and_label(item.get("publicationDate"), item.get("year"))
        url = item.get("url") or ""
        doi = (item.get("externalIds") or {}).get("DOI") or ""
        link = url or (f"https://doi.org/{doi}" if doi else "")
        out.append({
            "kind": "paper",
            "source": "Semantic",
            "topic": topic,
            "title": item.get("title", "") or "",
            "link": link,
            "date_label": label,
            "ts": ts,
            "summary": summary,
            "grounding": abstract,
        })
    return out


def _fetch(query, limit):
    """Network call. Returns the parsed JSON body or {"error": "..."}.

    Non-200 (e.g. 429 throttle) raises HTTPError; a 200 with a {message, code}
    envelope (no 'data' key) is handled by the caller.
    """
    headers = {"User-Agent": "research-tool/0.1"}
    api_key = os.environ.get("S2_API_KEY")
    if api_key:
        headers["x-api-key"] = api_key
    req = Request(S2_SEARCH.format(quote(query), int(limit)), headers=headers)
    try:
        with urlopen(req, timeout=20) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except HTTPError as e:
        return {"error": f"HTTP {e.code}: {e.reason}"}
    except URLError as e:
        return {"error": f"URLError: {e.reason}"}


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op == "search":
        limit = int(args.get("limit", 12))
        body = _fetch(args.get("query", ""), limit)
        if not isinstance(body, dict) or "data" not in body:
            # non-200 / throttle envelope ({message, code}) / unexpected shape
            msg = body.get("error") or body.get("message") if isinstance(body, dict) else None
            return {"error": msg or "Semantic Scholar returned no data"}
        recs = normalize(body, topic=args.get("topic", ""))[:limit]
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

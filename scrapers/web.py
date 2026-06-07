#!/usr/bin/env python3
"""Web search adapter (Serper or Tavily). Single-shot stdio contract (spec §4.0):
read one JSON request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  search {query, limit, topic} -> {"records": [...]}  (Paper-shaped, source="Web")

Key from SERPER_API_KEY (Google Serper) or TAVILY_API_KEY. With NEITHER set, returns
{"records": []} silently — web search is opt-in, so it doesn't pollute the feed with a
source error when unconfigured.
"""
import json
import os
import sys
from urllib.request import urlopen, Request

SERPER_URL = "https://google.serper.dev/search"
TAVILY_URL = "https://api.tavily.com/search"


def _rec(title, link, snippet, topic):
    snippet = snippet or ""
    return {
        "kind": "paper",
        "source": "Web",
        "topic": topic,
        "title": title or "",
        "link": link or "",
        "date_label": "",
        "ts": 0,
        "summary": snippet[:180],
        "grounding": snippet,
    }


def normalize_serper(data, topic):
    """Pure: Serper /search JSON -> Paper-shaped records."""
    return [
        _rec(o.get("title"), o.get("link"), o.get("snippet"), topic)
        for o in ((data or {}).get("organic") or [])
        if o.get("link")
    ]


def normalize_tavily(data, topic):
    """Pure: Tavily /search JSON -> Paper-shaped records."""
    return [
        _rec(r.get("title"), r.get("url"), r.get("content"), topic)
        for r in ((data or {}).get("results") or [])
        if r.get("url")
    ]


def _post(url, payload, headers):
    body = json.dumps(payload).encode("utf-8")
    req = Request(
        url,
        data=body,
        headers={**headers, "Content-Type": "application/json", "User-Agent": "jart/0.1"},
    )
    with urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op != "search":
        return {"error": f"unknown op: {op}"}
    query = args.get("query", "")
    limit = int(args.get("limit", 8))
    topic = args.get("topic", "")

    serper = os.environ.get("SERPER_API_KEY", "").strip()
    tavily = os.environ.get("TAVILY_API_KEY", "").strip()
    if serper:
        data = _post(SERPER_URL, {"q": query, "num": limit}, {"X-API-KEY": serper})
        return {"records": normalize_serper(data, topic)[:limit]}
    if tavily:
        data = _post(TAVILY_URL, {"api_key": tavily, "query": query, "max_results": limit}, {})
        return {"records": normalize_tavily(data, topic)[:limit]}
    return {"records": []}  # opt-in: no key -> silent empty


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

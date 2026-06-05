#!/usr/bin/env python3
"""HF papers/repos/spaces adapter. Single-shot stdio contract (spec §4.0): read
one JSON request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  paper_search {query, limit, topic} -> {"records": [...paper...]}
  repo_search  {query, repo_types:["model","dataset"], sort, limit}
               -> {"records": [{kind:"model"|"dataset", name, link, downloads, likes}]}
  space_search {query, sort, limit}
               -> {"records": [{name, link, likes, sdk}]}
"""
import json
import os
import sys
from datetime import datetime, timezone
from urllib.parse import quote, urlencode
from urllib.request import urlopen, Request

HF_SEARCH = "https://huggingface.co/api/papers/search?q={}"
HF_API = "https://huggingface.co/api/{}"  # models | datasets | spaces

# api path segment -> human/link path segment
_REPO_PATH = {"model": "models", "dataset": "datasets"}


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


def _str(v):
    """Coerce a count/field to a string (empty string for missing/None)."""
    if v is None:
        return ""
    return str(v)


def normalize_repos(raw, kind):
    """Pure: HF models/datasets JSON (list) -> list of Repo records.

    kind is "model" or "dataset". link = huggingface.co/{models|datasets}/{id}.
    downloads/likes are emitted as strings to match the Rust Repo wire shape.
    """
    seg = _REPO_PATH.get(kind, kind + "s")
    out = []
    for item in raw or []:
        it = item or {}
        rid = it.get("id") or it.get("modelId") or ""
        out.append({
            "kind": kind,
            "name": rid,
            "link": f"https://huggingface.co/{seg}/{rid}" if rid else "",
            "downloads": _str(it.get("downloads", "")),
            "likes": _str(it.get("likes", "")),
        })
    return out


def normalize_spaces(raw):
    """Pure: HF spaces JSON (list) -> list of Space records.

    link = huggingface.co/spaces/{id}. likes emitted as a string.
    """
    out = []
    for item in raw or []:
        it = item or {}
        sid = it.get("id") or ""
        out.append({
            "name": sid,
            "link": f"https://huggingface.co/spaces/{sid}" if sid else "",
            "likes": _str(it.get("likes", "")),
            "sdk": it.get("sdk") or "",
        })
    return out


def _fetch(query):
    req = Request(HF_SEARCH.format(quote(query)), headers=_headers())
    with urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _headers():
    h = {"User-Agent": "research-tool/0.1"}
    token = os.environ.get("HF_TOKEN")
    if token:
        h["Authorization"] = f"Bearer {token}"
    return h


def _fetch_api(kind, query, sort, limit):
    """Fetch huggingface.co/api/{models|datasets|spaces} as a list."""
    seg = _REPO_PATH.get(kind, kind + "s") if kind in _REPO_PATH else kind
    params = {"search": query, "sort": sort, "limit": limit}
    url = HF_API.format(seg) + "?" + urlencode(params)
    req = Request(url, headers=_headers())
    with urlopen(req, timeout=20) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return data if isinstance(data, list) else []


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op == "paper_search":
        raw = _fetch(args.get("query", ""))
        recs = normalize(raw, topic=args.get("topic", ""))[: int(args.get("limit", 12))]
        return {"records": recs}
    if op == "repo_search":
        query = args.get("query", "")
        sort = args.get("sort", "trendingScore")
        limit = int(args.get("limit", 12))
        repo_types = args.get("repo_types") or ["model", "dataset"]
        recs = []
        for kind in repo_types:
            raw = _fetch_api(kind, query, sort, limit)
            recs.extend(normalize_repos(raw, kind)[:limit])
        return {"records": recs}
    if op == "space_search":
        raw = _fetch_api("spaces", args.get("query", ""), args.get("sort", "likes"),
                         int(args.get("limit", 12)))
        recs = normalize_spaces(raw)[: int(args.get("limit", 12))]
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

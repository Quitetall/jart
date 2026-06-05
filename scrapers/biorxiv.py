#!/usr/bin/env python3
"""bioRxiv/medRxiv preprint adapter. Single-shot stdio contract (spec §4.0):
read one JSON request {op, args} from stdin to EOF, write one JSON object to
stdout, exit 0 on any exception.

The bioRxiv details API has no keyword search: it serves a date-range
collection. We fetch a recent window and filter client-side by query keywords.

Ops:
  search    {query, limit, topic, servers?, today?} -> {"records": [...]}
  published {query, limit, topic, servers?, today?} -> {"records": [...]}
"""
import json
import re
import sys
from datetime import datetime, timedelta, timezone
from urllib.request import urlopen, Request

DETAILS = "https://api.biorxiv.org/details/{server}/{frm}/{to}/{cursor}"
WINDOW_DAYS = 45
PAGE = 30                       # collection page size per the API
DEFAULT_SERVERS = ["biorxiv", "medrxiv"]
MAX_PAGES = 200                 # hard stop so a runaway cursor can't loop forever
# The window holds thousands of preprints across all categories; /details is
# oldest-first, so we fetch only the most-recent tail (≈ RECENT_PAGES*PAGE items)
# instead of paging through the whole window (which took ~90s).
RECENT_PAGES = 4


def date_window(today=None):
    """Pure: (from, to) ISO dates for the recent WINDOW_DAYS window.

    `today` is an optional 'YYYY-MM-DD' string (deterministic tests); else the
    real current UTC date. Returns (from_str, to_str).
    """
    if today:
        end = datetime.strptime(today, "%Y-%m-%d").replace(tzinfo=timezone.utc)
    else:
        end = datetime.now(timezone.utc)
    start = end - timedelta(days=WINDOW_DAYS)
    return start.strftime("%Y-%m-%d"), end.strftime("%Y-%m-%d")


def matches(text, query):
    """Pure relevance gate for the date-range window (bioRxiv has no keyword
    search, so this filters an all-category recent collection). The FIRST query
    token is the domain anchor and MUST appear as a whole word — generic ML
    tokens like "detection"/"deep"/"learning" can't pull in off-topic preprints
    (a deep-learning paper about parrots no longer matches an "EEG ..." query).
    Word-boundary anchored (no "deep"→"deepen"). Empty query matches everything.
    Put the most distinctive domain term first in a topic's query."""
    tokens = [t for t in (query or "").split() if t]
    if not tokens:
        return True
    anchor = tokens[0]
    return re.search(r"\b" + re.escape(anchor) + r"\b", text or "", re.IGNORECASE) is not None


def _ts_and_label(date_str):
    """bioRxiv `date` is 'YYYY-MM-DD' (no time). Anchor at UTC midnight so the
    epoch-millis ts is machine-independent."""
    if not date_str:
        return 0, ""
    try:
        dt = datetime.strptime(date_str, "%Y-%m-%d").replace(tzinfo=timezone.utc)
        return int(dt.timestamp() * 1000), dt.strftime("%Y-%m-%d")
    except (ValueError, TypeError):
        return 0, ""


def normalize(collection, topic=""):
    """Pure: bioRxiv collection (list of details dicts) -> Paper records.

    No filtering here; the op handlers decide which items to keep. The `doi`
    field drives the link, so the `published` handler can remap `doi` to the
    published DOI before calling this.
    """
    out = []
    for it in collection or []:
        it = it or {}
        doi = (it.get("doi") or "").strip()
        abstract = it.get("abstract") or ""
        ts, label = _ts_and_label(it.get("date") or "")
        out.append({
            "kind": "paper",
            "source": "Preprint",
            "topic": topic,
            "title": it.get("title", "") or "",
            "link": f"https://doi.org/{doi}" if doi else "",
            "date_label": label,
            "ts": ts,
            "summary": (abstract[:180] + "…") if abstract else "",
            "grounding": abstract,
        })
    return out


def _get_page(server, frm, to, cursor):
    url = DETAILS.format(server=server, frm=frm, to=to, cursor=cursor)
    req = Request(url, headers={"User-Agent": "research-tool/0.1"})
    with urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _fetch_collection(server, frm, to):
    """Fetch the most-recent tail of the date-range window.

    `/details` is oldest-first within the range and the window spans thousands
    of preprints across all categories, so we probe `total` once, then read only
    the last `RECENT_PAGES` pages (the newest items). Bounds the work to ~5 HTTP
    requests instead of paging the entire window.
    """
    body = _get_page(server, frm, to, 0)
    messages = body.get("messages") or [{}]
    try:
        total = int(messages[0].get("total", 0))
    except (TypeError, ValueError):
        total = 0
    first_page = body.get("collection") or []

    # Small window: the first page already covers (most of) it.
    if total <= PAGE * RECENT_PAGES:
        collection = list(first_page)
        cursor = PAGE
        while cursor < total and cursor < PAGE * MAX_PAGES:
            pg = _get_page(server, frm, to, cursor).get("collection") or []
            if not pg:
                break
            collection.extend(pg)
            cursor += PAGE
        return collection

    # Large window: read the last RECENT_PAGES pages (most recent items).
    last_cursor = (total // PAGE) * PAGE
    cursor = max(0, last_cursor - (RECENT_PAGES - 1) * PAGE)
    collection = []
    while cursor <= last_cursor:
        try:
            pg = _get_page(server, frm, to, cursor).get("collection") or []
        except Exception:
            break
        if not pg:
            break
        collection.extend(pg)
        cursor += PAGE
    return collection


def _gather(args):
    """Fetch the merged collection across the requested servers."""
    servers = args.get("servers") or DEFAULT_SERVERS
    frm, to = date_window(args.get("today"))
    merged = []
    for server in servers:
        merged.extend(_fetch_collection(server, frm, to))
    return merged


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    topic = args.get("topic", "")
    query = args.get("query", "")
    limit = int(args.get("limit", 12))

    if op == "search":
        collection = _gather(args)
        kept = [
            it for it in collection
            if matches(f"{(it or {}).get('title', '')} {(it or {}).get('abstract', '')}", query)
        ]
        return {"records": normalize(kept, topic)[:limit]}

    if op == "published":
        collection = _gather(args)
        # Keep items with a real published DOI; map the link to that DOI by
        # swapping the `doi` field before normalize (keeps normalize signature).
        remapped = [
            {**it, "doi": it.get("published")}
            for it in collection
            if (it or {}).get("published") and it.get("published") != "NA"
        ]
        return {"records": normalize(remapped, topic)[:limit]}

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

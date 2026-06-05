#!/usr/bin/env python3
"""PubMed E-utilities adapter. Single-shot stdio contract (spec §4.0): read one
JSON request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  search {query, limit, topic} -> {"records": [...]}

Pipeline: esearch (json, sort=date) -> pmids -> efetch (xml, rettype=abstract)
-> parse title/abstract/journal/PubDate/DOI. Honors NCBI_API_KEY env.
"""
import json
import os
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from urllib.parse import quote
from urllib.request import urlopen, Request

ESEARCH = (
    "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi"
    "?db=pubmed&term={}&retmax={}&retmode=json&sort=date"
)
EFETCH = (
    "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/efetch.fcgi"
    "?db=pubmed&id={}&retmode=xml&rettype=abstract"
)

_MONTHS = {
    "jan": 1, "feb": 2, "mar": 3, "apr": 4, "may": 5, "jun": 6,
    "jul": 7, "aug": 8, "sep": 9, "oct": 10, "nov": 11, "dec": 12,
}


def _text(el):
    """Full text content of an element, including nested tags (<i>, <sup>...)."""
    if el is None:
        return ""
    return "".join(el.itertext()).strip()


def _month_num(raw):
    if not raw:
        return 1
    raw = raw.strip()
    try:
        return int(raw)
    except ValueError:
        return _MONTHS.get(raw[:3].lower(), 1)


def _pubdate(pubdate_el):
    """PubDate (Year/Month/Day) -> (date_label 'YYYY-MM-DD', ts epoch millis).

    Returns ("", 0) when no Year is present (e.g. MedlineDate-only records).
    """
    if pubdate_el is None:
        return "", 0
    year_el = pubdate_el.find("Year")
    if year_el is None or not (year_el.text or "").strip():
        return "", 0
    try:
        year = int(year_el.text.strip())
    except ValueError:
        return "", 0
    month_el = pubdate_el.find("Month")
    day_el = pubdate_el.find("Day")
    month = _month_num(month_el.text if month_el is not None else None)
    try:
        day = int(day_el.text.strip()) if day_el is not None and (day_el.text or "").strip() else 1
    except ValueError:
        day = 1
    try:
        dt = datetime(year, month, day, tzinfo=timezone.utc)
    except ValueError:
        return "", 0
    return dt.strftime("%Y-%m-%d"), int(dt.timestamp() * 1000)


def _summary(abstract):
    if len(abstract) <= 180:
        return abstract
    return abstract[:180] + "…"


def parse(xml_text, topic=""):
    """Pure: efetch PubMed XML -> list of normalized Paper records."""
    out = []
    if not xml_text or not xml_text.strip():
        return out
    root = ET.fromstring(xml_text)
    for art in root.findall("PubmedArticle"):
        citation = art.find("MedlineCitation")
        if citation is None:
            continue
        pmid_el = citation.find("PMID")
        pmid = _text(pmid_el)
        article = citation.find("Article")
        if article is None:
            continue

        title = _text(article.find("ArticleTitle"))

        # Multiple AbstractText sections (structured abstracts) -> join.
        parts = [_text(at) for at in article.findall("Abstract/AbstractText")]
        abstract = " ".join(p for p in parts if p)

        date_label, ts = _pubdate(article.find("Journal/JournalIssue/PubDate"))

        doi_el = art.find('.//ArticleId[@IdType="doi"]')
        doi = _text(doi_el)
        if doi:
            link = f"https://doi.org/{doi}"
        elif pmid:
            link = f"https://pubmed.ncbi.nlm.nih.gov/{pmid}"
        else:
            link = ""

        out.append({
            "kind": "paper",
            "source": "PubMed",
            "topic": topic,
            "title": title,
            "link": link,
            "date_label": date_label,
            "ts": ts,
            "summary": _summary(abstract),
            "grounding": abstract,
        })
    return out


def _api_key_suffix():
    key = os.environ.get("NCBI_API_KEY", "").strip()
    return f"&api_key={quote(key)}" if key else ""


def _get(url):
    req = Request(url, headers={"User-Agent": "research-tool/0.1"})
    with urlopen(req, timeout=20) as resp:
        return resp.read().decode("utf-8")


def _esearch(query, limit):
    url = ESEARCH.format(quote(query), int(limit)) + _api_key_suffix()
    data = json.loads(_get(url))
    return ((data or {}).get("esearchresult") or {}).get("idlist") or []


def _efetch(pmids):
    url = EFETCH.format(",".join(pmids)) + _api_key_suffix()
    return _get(url)


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op == "search":
        limit = int(args.get("limit", 12))
        pmids = _esearch(args.get("query", ""), limit)[:limit]
        if not pmids:
            return {"records": []}
        # This request makes two NCBI calls (esearch + efetch). Unkeyed, the limit
        # is 3 req/s; space the pair so concurrent topics stay under it.
        if not os.environ.get("NCBI_API_KEY", "").strip():
            time.sleep(0.35)
        xml_text = _efetch(pmids)
        return {"records": parse(xml_text, topic=args.get("topic", ""))[:limit]}
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

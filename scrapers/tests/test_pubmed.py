import json, pathlib, sys
from datetime import datetime, timezone

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import pubmed

FIX = pathlib.Path(__file__).parent / "fixtures" / "pubmed_efetch.xml"


def _recs():
    return pubmed.parse(FIX.read_text(), topic="EEG foundation models")


def test_parse_maps_core_fields():
    recs = _recs()
    assert len(recs) == 2
    a = recs[0]
    assert a["kind"] == "paper"
    assert a["source"] == "PubMed"
    assert a["topic"] == "EEG foundation models"
    # nested <i> tag preserved via itertext, not truncated at first child
    assert a["title"] == "A self-supervised EEG foundation model"
    # exactly the Paper wire shape — no extra fields
    assert set(a.keys()) == {
        "kind", "source", "topic", "title", "link",
        "date_label", "ts", "summary", "grounding",
    }


def test_parse_month_name_to_date_and_ts():
    a = _recs()[0]
    assert a["date_label"] == "2026-05-12"
    expected_ts = int(datetime(2026, 5, 12, tzinfo=timezone.utc).timestamp() * 1000)
    assert a["ts"] == expected_ts


def test_parse_doi_link():
    a = _recs()[0]
    assert a["link"] == "https://doi.org/10.1088/1741-2552/abcd12"


def test_parse_joins_structured_abstract_and_keeps_nested_text():
    a = _recs()[0]
    # all three labeled sections joined; nested <sup>large</sup> preserved
    assert "hand-crafted features" in a["grounding"]
    assert "large unlabeled EEG corpora" in a["grounding"]
    assert "seizure detection by a wide margin" in a["grounding"]
    assert a["grounding"] != ""


def test_summary_truncated_grounding_full():
    a = _recs()[0]
    # long abstract: summary is truncated form, grounding is full text
    assert len(a["grounding"]) > 180
    assert a["summary"] == a["grounding"][:180] + "…"
    assert a["summary"].endswith("…")
    assert a["summary"] != a["grounding"]


def test_parse_year_only_fallback_and_pubmed_link():
    b = _recs()[1]
    # year-only PubDate -> Jan 1 default
    assert b["date_label"] == "2025-01-01"
    assert b["ts"] == int(datetime(2025, 1, 1, tzinfo=timezone.utc).timestamp() * 1000)
    # no DOI -> pubmed url fallback
    assert b["link"] == "https://pubmed.ncbi.nlm.nih.gov/40987654"
    # short single-section abstract: summary == grounding (<=180)
    assert b["summary"] == b["grounding"]
    assert b["grounding"] != ""


def test_parse_empty_xml_returns_empty():
    assert pubmed.parse("", topic="t") == []


def test_handle_unknown_op():
    assert "error" in pubmed.handle({"op": "nope"})


def test_handle_search_empty_idlist_skips_efetch(monkeypatch):
    monkeypatch.setattr(pubmed, "_esearch", lambda q, n: [])
    # _efetch should never be called; make it explode if it is
    monkeypatch.setattr(pubmed, "_efetch", lambda ids: 1 / 0)
    assert pubmed.handle({"op": "search", "args": {"query": "x"}}) == {"records": []}

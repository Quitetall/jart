import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import semantic

FIX = pathlib.Path(__file__).parent / "fixtures" / "semantic.json"


def test_normalize_maps_fields_and_dates():
    raw = json.loads(FIX.read_text())
    recs = semantic.normalize(raw, topic="EEG")
    assert len(recs) == 2

    a = recs[0]
    assert a["source"] == "Semantic"
    assert a["kind"] == "paper"
    assert a["topic"] == "EEG"
    assert a["title"] == "An EEG Transformer for Seizure Forecasting"
    # url present -> link is the url (not the doi fallback)
    assert a["link"] == "https://www.semanticscholar.org/paper/abc123"
    assert a["date_label"] == "2025-03-15"
    assert a["ts"] == 1741996800000          # 2025-03-15T00:00:00Z in ms
    assert a["summary"].endswith("…")
    assert a["grounding"].startswith("We present a transformer")


def test_normalize_doi_fallback_year_only_and_null_abstract():
    raw = json.loads(FIX.read_text())
    recs = semantic.normalize(raw, topic="EEG")
    b = recs[1]
    # no url -> doi.org fallback from externalIds.DOI
    assert b["link"] == "https://doi.org/10.5678/eeg.2024.042"
    # no publicationDate -> year-only label + Jan 1 ts
    assert b["date_label"] == "2024"
    assert b["ts"] == 1704067200000          # 2024-01-01T00:00:00Z in ms
    # null abstract -> empty summary/grounding, no crash
    assert b["summary"] == ""
    assert b["grounding"] == ""


def test_error_envelope_normalizes_to_empty_list():
    # The throttle envelope S2 returns instead of {data:[...]}.
    envelope = {"message": "Too Many Requests", "code": "429"}
    assert semantic.normalize(envelope, topic="EEG") == []


def test_empty_and_missing_data_normalize_to_empty_list():
    assert semantic.normalize({"data": []}, topic="EEG") == []
    assert semantic.normalize({}, topic="EEG") == []
    assert semantic.normalize(None, topic="EEG") == []


def test_handle_missing_data_returns_error(monkeypatch):
    # _fetch yields a throttle envelope -> handle must return {"error": ...}
    monkeypatch.setattr(semantic, "_fetch", lambda q, l: {"message": "Too Many Requests", "code": "429"})
    out = semantic.handle({"op": "search", "args": {"query": "eeg", "limit": 5, "topic": "EEG"}})
    assert "error" in out and "records" not in out


def test_handle_search_returns_records(monkeypatch):
    raw = json.loads(FIX.read_text())
    monkeypatch.setattr(semantic, "_fetch", lambda q, l: raw)
    out = semantic.handle({"op": "search", "args": {"query": "eeg", "limit": 1, "topic": "EEG"}})
    assert "records" in out
    assert len(out["records"]) == 1          # limit honored
    assert out["records"][0]["source"] == "Semantic"


def test_unknown_op_returns_error():
    out = semantic.handle({"op": "nope", "args": {}})
    assert "error" in out

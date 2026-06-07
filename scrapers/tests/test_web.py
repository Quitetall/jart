import json, os, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import web


def test_normalize_serper_maps_fields():
    data = {"organic": [
        {"title": "EEG decoding review", "link": "https://x/1", "snippet": "A review of EEG decoding."},
        {"title": "no link dropped", "snippet": "x"},  # no link -> dropped
    ]}
    recs = web.normalize_serper(data, "Seizure")
    assert len(recs) == 1
    r = recs[0]
    assert r["source"] == "Web"
    assert r["topic"] == "Seizure"
    assert r["title"] == "EEG decoding review"
    assert r["link"] == "https://x/1"
    assert r["grounding"] == "A review of EEG decoding."


def test_normalize_tavily_maps_fields():
    data = {"results": [{"title": "T", "url": "https://y/2", "content": "body text"}]}
    recs = web.normalize_tavily(data, "t")
    assert recs[0]["link"] == "https://y/2"
    assert recs[0]["grounding"] == "body text"
    assert recs[0]["source"] == "Web"


def test_handle_no_key_returns_empty_silently(monkeypatch):
    monkeypatch.delenv("SERPER_API_KEY", raising=False)
    monkeypatch.delenv("TAVILY_API_KEY", raising=False)
    assert web.handle({"op": "search", "args": {"query": "EEG"}}) == {"records": []}


def test_handle_unknown_op():
    assert "error" in web.handle({"op": "nope"})

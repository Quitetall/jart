import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import biorxiv

FIX = pathlib.Path(__file__).parent / "fixtures" / "biorxiv.json"


def _coll():
    return json.loads(FIX.read_text())


def test_normalize_maps_fields():
    recs = biorxiv.normalize(_coll(), topic="Seizure detection")
    assert len(recs) == 2
    a = recs[0]
    assert a["kind"] == "paper"
    assert a["source"] == "Preprint"
    assert a["topic"] == "Seizure detection"
    assert a["title"] == "An EEG Foundation Model for Seizure Detection"
    assert a["link"] == "https://doi.org/10.1101/2026.05.01.111111"
    assert a["date_label"] == "2026-05-01"
    assert a["ts"] == 1777593600000  # 2026-05-01T00:00:00Z in ms (UTC midnight)
    assert a["summary"].endswith("…")
    assert a["grounding"].startswith("We pretrain a transformer")


def test_normalize_tolerates_missing_fields():
    recs = biorxiv.normalize([{"doi": ""}], topic="t")
    r = recs[0]
    assert r["title"] == ""
    assert r["link"] == ""        # no doi -> empty link
    assert r["ts"] == 0           # no date -> 0
    assert r["summary"] == ""
    assert r["grounding"] == ""


def test_matches_keyword_hit_and_miss():
    coll = _coll()
    eeg = coll[0]["title"] + " " + coll[0]["abstract"]
    protein = coll[1]["title"] + " " + coll[1]["abstract"]
    # first token is the required domain anchor (whole word)
    assert biorxiv.matches(eeg, "seizure EEG") is True            # anchor "seizure" present
    assert biorxiv.matches(protein, "seizure EEG") is False       # anchor "seizure" absent
    # generic trailing tokens can't rescue a missing anchor
    assert biorxiv.matches(protein, "seizure folding") is False   # anchor "seizure" absent
    assert biorxiv.matches(protein, "folding seizure") is True    # anchor "folding" present
    # word-boundary: a substring of the anchor does not count
    assert biorxiv.matches("a deepen study", "deep learning") is False
    # empty query keeps everything
    assert biorxiv.matches(protein, "") is True


def test_date_window_is_deterministic_with_today():
    frm, to = biorxiv.date_window("2026-06-05")
    assert to == "2026-06-05"
    assert frm == "2026-04-21"   # 45 days before 2026-06-05


def test_search_op_filters_by_query():
    req = {"op": "search", "args": {"query": "seizure EEG", "topic": "Seizure", "limit": 12}}
    # monkeypatch the network gather to return the fixture collection
    biorxiv._gather = lambda args: _coll()
    out = biorxiv.handle(req)
    assert "records" in out
    recs = out["records"]
    assert len(recs) == 1                       # protein-folding item filtered out
    assert recs[0]["title"].startswith("An EEG Foundation Model")
    assert recs[0]["source"] == "Preprint"


def test_search_op_respects_limit():
    biorxiv._gather = lambda args: _coll()
    out = biorxiv.handle({"op": "search", "args": {"query": "", "limit": 1}})
    assert len(out["records"]) == 1             # empty query keeps both, cap to 1


def test_published_op_keeps_published_and_links_to_published_doi():
    biorxiv._gather = lambda args: _coll()
    out = biorxiv.handle({"op": "published", "args": {"query": "", "topic": "T", "limit": 12}})
    recs = out["records"]
    assert len(recs) == 1                       # NA item dropped
    r = recs[0]
    # link must point at the PUBLISHED DOI, not the preprint DOI
    assert r["link"] == "https://doi.org/10.1038/s41586-026-00001-2"
    assert r["source"] == "Preprint"
    assert r["topic"] == "T"


def test_handle_unknown_op():
    assert "error" in biorxiv.handle({"op": "bogus", "args": {}})

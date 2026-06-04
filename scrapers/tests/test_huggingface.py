import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import huggingface

FIX = pathlib.Path(__file__).parent / "fixtures" / "hf_papers.json"

def test_normalize_maps_fields_and_prefers_ai_summary():
    raw = json.loads(FIX.read_text())
    recs = huggingface.normalize(raw, topic="Foundation models")
    assert len(recs) == 2
    a = recs[0]
    assert a["source"] == "HF"
    assert a["kind"] == "paper"
    assert a["topic"] == "Foundation models"
    assert a["title"] == "An EEG Foundation Model"
    assert a["link"] == "https://huggingface.co/papers/2405.12345"
    assert a["date_label"] == "2026-05-01"
    assert a["ts"] == 1777593600000          # 2026-05-01T00:00:00Z in ms
    assert a["summary"] == "Pretrains a transformer on large EEG corpora."
    assert recs[1]["summary"] == "A CNN detects seizures."  # no ai_summary -> fallback

def test_normalize_tolerates_missing_fields():
    recs = huggingface.normalize([{"paper": {"id": "x"}}], topic="t")
    r = recs[0]
    assert r["title"] == ""
    assert r["ts"] == 0
    assert r["link"] == "https://huggingface.co/papers/x"

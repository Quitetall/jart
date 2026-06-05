import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import huggingface

FIXDIR = pathlib.Path(__file__).parent / "fixtures"
FIX = FIXDIR / "hf_papers.json"

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


def test_normalize_repos_models():
    raw = json.loads((FIXDIR / "hf_models.json").read_text())
    recs = huggingface.normalize_repos(raw, "model")
    assert len(recs) == 3
    a = recs[0]
    assert a["kind"] == "model"
    assert a["name"] == "openbmb/eeg-foundation"
    assert a["link"] == "https://huggingface.co/models/openbmb/eeg-foundation"
    assert a["downloads"] == "12345"      # coerced to string
    assert a["likes"] == "42"
    # id falls back to modelId
    assert recs[1]["name"] == "Dseid/EEG_classifier"
    assert recs[1]["downloads"] == "0"
    # missing id -> empty name + empty link, no crash
    assert recs[2]["name"] == ""
    assert recs[2]["link"] == ""
    assert recs[2]["downloads"] == ""
    assert recs[2]["likes"] == ""


def test_normalize_repos_datasets():
    raw = json.loads((FIXDIR / "hf_datasets.json").read_text())
    recs = huggingface.normalize_repos(raw, "dataset")
    assert len(recs) == 2
    a = recs[0]
    assert a["kind"] == "dataset"
    assert a["name"] == "neurofusion/eeg-restingstate"
    assert a["link"] == "https://huggingface.co/datasets/neurofusion/eeg-restingstate"
    assert a["downloads"] == "33"
    assert a["likes"] == "7"


def test_normalize_spaces():
    raw = json.loads((FIXDIR / "hf_spaces.json").read_text())
    recs = huggingface.normalize_spaces(raw)
    assert len(recs) == 3
    a = recs[0]
    assert a["name"] == "JavadBayazi/EEG_cls"
    assert a["link"] == "https://huggingface.co/spaces/JavadBayazi/EEG_cls"
    assert a["likes"] == "4"
    assert a["sdk"] == "streamlit"
    assert recs[1]["sdk"] == "gradio"
    # missing sdk/id -> empty strings, no crash
    assert recs[2]["sdk"] == ""
    assert recs[2]["name"] == "noSdk/space"


def test_handle_unknown_op():
    assert "error" in huggingface.handle({"op": "nope"})


def test_repo_records_have_only_expected_keys():
    recs = huggingface.normalize_repos([{"id": "a/b"}], "model")
    assert set(recs[0].keys()) == {"kind", "name", "link", "downloads", "likes"}
    srecs = huggingface.normalize_spaces([{"id": "a/b", "sdk": "docker"}])
    assert set(srecs[0].keys()) == {"name", "link", "likes", "sdk"}

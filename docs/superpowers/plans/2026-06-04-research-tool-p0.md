# research Tool — P0 Walking Skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the full integration spine end-to-end — HF papers → Python adapter (stdio) → Rust core → axum `/api` → TypeScript card, plus one LAMU-backed AI summary.

**Architecture:** Single Rust binary (`research`) owns an axum web server + orchestration. A stateless Python adapter fetches HF papers and returns normalized JSON over stdio (single-shot framing: write request, close stdin, read one JSON object back). AI summaries go to the local LAMU OpenAI-compat HTTP endpoint (`/v1/chat/completions`, default model `mimo-v2.5`). A Vite + TypeScript frontend renders the feed and triggers a summary. The frontend builds DOM nodes with `createElement` + `textContent` (no HTML-string injection) so untrusted paper titles/URLs cannot inject script.

**Tech Stack:** Rust (tokio, axum, tower-http, reqwest, serde, serde_json, clap, anyhow; dev: wiremock), Python 3 (stdlib `urllib` only for P0), TypeScript + Vite (dev: vitest, jsdom).

**Reference spec:** `docs/superpowers/specs/2026-06-04-research-tool-design.md` (§4.0 stdio framing, §4.2 AI surface, §5 sources, §6 config, §10 build order).

**Prerequisite:** LAMU reachable at `http://localhost:8020` (run `lamu serve` in another terminal) for the live AI smoke step only. All unit tests stub it.

---

## File Structure

```
~/Desktop/research/
  Cargo.toml                     # bin crate `research`
  src/
    main.rs                      # clap CLI entry; `research`, `research --check`
    lib.rs                       # pub mod core; pub mod server;
    server.rs                    # axum router: /api/feed, /api/summary, static
    core/
      mod.rs                     # pub mod model; config; scrape; ai; feed;
      model.rs                   # Paper, Feed, SourceError
      config.rs                  # Config, Topic, EEG/BCI default preset
      scrape.rs                  # spawn adapter, single-shot stdio framing
      ai.rs                      # LAMU /v1/chat/completions client
      feed.rs                    # orchestrate sources -> Feed (P0: HF only)
  scrapers/
    huggingface.py               # op=paper_search -> normalized JSON
    tests/
      test_huggingface.py        # pure normalize() over a fixture
      fixtures/hf_papers.json    # recorded HF API response
  tests/
    fixtures/echo_adapter.py     # fake adapter for scrape.rs tests
    fixtures/feed/huggingface.py # echo aliased as "huggingface" for feed tests
  frontend/
    index.html
    package.json
    tsconfig.json
    vite.config.ts
    src/
      types.ts                   # Paper interface
      api.ts                     # typed fetch client for /api/*
      render.ts                  # pure feed -> DOM nodes (no innerHTML)
      main.ts                    # boot: fetch feed, render, wire summary
      render.test.ts             # vitest (jsdom) over render.ts
```

---

## Task 1: Rust scaffold + model types

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/core/mod.rs`, `src/core/model.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "research"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "research"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.7"
tower-http = { version = "0.6", features = ["fs"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
toml = "0.8"

[dev-dependencies]
wiremock = "0.6"
```

- [ ] **Step 2: Write the failing test in `src/core/model.rs`**

```rust
//! Core data shapes shared by the TUI, the web API, and the scrapers.

use serde::{Deserialize, Serialize};

/// One research item. JSON field names are the wire contract with both the
/// Python adapters and the TS frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    pub kind: String,        // "paper"
    pub source: String,      // "HF" | "PubMed" | "Preprint" | "Consensus"
    pub topic: String,
    pub title: String,
    pub link: String,
    pub date_label: String,
    pub ts: i64,             // epoch millis, 0 if unknown
    pub summary: String,
    pub grounding: String,
}

/// A source that failed to load. Surfaced per-panel in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceError {
    pub source: String,
    pub message: String,
}

/// Aggregated result of one feed load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feed {
    pub papers: Vec<Paper>,
    pub errors: Vec<SourceError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_json_roundtrips_with_snake_case_fields() {
        let p = Paper {
            kind: "paper".into(),
            source: "HF".into(),
            topic: "Seizure detection".into(),
            title: "A Test Paper".into(),
            link: "https://huggingface.co/papers/1234.5678".into(),
            date_label: "2026-05-01".into(),
            ts: 1_746_057_600_000,
            summary: "short".into(),
            grounding: "longer".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"date_label\""), "wire field must be date_label");
        let back: Paper = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
```

- [ ] **Step 3: Create `src/core/mod.rs`**

```rust
pub mod model;
```

- [ ] **Step 4: Create `src/lib.rs`**

```rust
pub mod core;
// pub mod server;  // uncommented in Task 6
```

- [ ] **Step 5: Create `src/main.rs`**

```rust
fn main() {
    println!("research — see `research --check` (P0 scaffold)");
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib core::model`
Expected: PASS — `paper_json_roundtrips_with_snake_case_fields ... ok`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/
git commit -m "feat: rust scaffold + core model types"
```

---

## Task 2: Python HF papers adapter (pure normalize + fixture test)

**Files:**
- Create: `scrapers/huggingface.py`, `scrapers/tests/test_huggingface.py`, `scrapers/tests/fixtures/hf_papers.json`

- [ ] **Step 1: Create the fixture `scrapers/tests/fixtures/hf_papers.json`**

```json
[
  {
    "paper": {
      "id": "2405.12345",
      "title": "An EEG Foundation Model",
      "publishedAt": "2026-05-01T00:00:00.000Z",
      "summary": "We pretrain a transformer on EEG.",
      "ai_summary": "Pretrains a transformer on large EEG corpora."
    }
  },
  {
    "paper": {
      "id": "2405.99999",
      "title": "Seizure Detection With CNNs",
      "publishedAt": "2026-04-15T00:00:00.000Z",
      "summary": "A CNN detects seizures."
    }
  }
]
```

- [ ] **Step 2: Write the failing test `scrapers/tests/test_huggingface.py`**

```python
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
    assert a["ts"] == 1746057600000          # 2026-05-01T00:00:00Z in ms
    assert a["summary"] == "Pretrains a transformer on large EEG corpora."
    assert recs[1]["summary"] == "A CNN detects seizures."  # no ai_summary -> fallback

def test_normalize_tolerates_missing_fields():
    recs = huggingface.normalize([{"paper": {"id": "x"}}], topic="t")
    r = recs[0]
    assert r["title"] == ""
    assert r["ts"] == 0
    assert r["link"] == "https://huggingface.co/papers/x"
```

- [ ] **Step 3: Run to verify it fails**

Run: `cd scrapers && python -m pytest tests/test_huggingface.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'huggingface'`.

- [ ] **Step 4: Implement `scrapers/huggingface.py`**

```python
#!/usr/bin/env python3
"""HF papers adapter. Single-shot stdio contract (spec §4.0): read one JSON
request {op, args} from stdin to EOF, write one JSON object to stdout.

Ops:
  paper_search {query, limit, topic} -> {"records": [...]}
"""
import json
import sys
from datetime import datetime, timezone
from urllib.parse import quote
from urllib.request import urlopen, Request

HF_SEARCH = "https://huggingface.co/api/papers/search?q={}"


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


def _fetch(query):
    req = Request(HF_SEARCH.format(quote(query)), headers={"User-Agent": "research-tool/0.1"})
    with urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def handle(request):
    op = request.get("op")
    args = request.get("args") or {}
    if op == "paper_search":
        raw = _fetch(args.get("query", ""))
        recs = normalize(raw, topic=args.get("topic", ""))[: int(args.get("limit", 12))]
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
```

- [ ] **Step 5: Run to verify it passes**

Run: `cd scrapers && python -m pytest tests/test_huggingface.py -q`
Expected: PASS — 2 passed.

- [ ] **Step 6: Commit**

```bash
git add scrapers/
git commit -m "feat: HF papers python adapter with normalize() + fixture test"
```

---

## Task 3: Rust scrape.rs — single-shot stdio framing

**Files:**
- Create: `src/core/scrape.rs`, `tests/fixtures/echo_adapter.py`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Create the fake adapter `tests/fixtures/echo_adapter.py`**

```python
#!/usr/bin/env python3
# Test double: echo a fixed records payload regardless of input.
import json, sys
_ = sys.stdin.read()
sys.stdout.write(json.dumps({"records": [
    {"kind": "paper", "source": "HF", "topic": "t", "title": "Echo",
     "link": "https://example.com/x", "date_label": "2026-01-01",
     "ts": 1, "summary": "s", "grounding": "g"}
]}))
```

- [ ] **Step 2: Write the failing test in `src/core/scrape.rs`**

```rust
//! Spawn a Python adapter and exchange one JSON message over stdio.
//! Framing (spec §4.0): write request to stdin, close stdin, read stdout to EOF.

use crate::core::model::Paper;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Run `<scrapers_dir>/<source>.py`, sending `request`, returning parsed JSON.
pub async fn run_adapter(scrapers_dir: &Path, source: &str, request: &Value) -> Result<Value> {
    // Defense-in-depth: `source` names a sibling adapter, never a path.
    // Reject anything that could escape `scrapers_dir` (P1 adds dynamic sources).
    if !source.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(anyhow!("invalid adapter name: {source:?}"));
    }
    let script = scrapers_dir.join(format!("{source}.py"));
    let mut child = Command::new("python3")
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn adapter {}", script.display()))?;

    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    stdin.write_all(request.to_string().as_bytes()).await?;
    drop(stdin); // close -> EOF so the adapter's stdin.read() returns

    let out = child.wait_with_output().await?;
    if !out.status.success() {
        return Err(anyhow!(
            "adapter {source} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout)
        .with_context(|| format!("adapter {source} returned non-JSON"))
}

/// Convenience: run an adapter op and decode `records` into `Vec<Paper>`.
pub async fn fetch_papers(
    scrapers_dir: &Path,
    source: &str,
    op: &str,
    args: Value,
) -> Result<Vec<Paper>> {
    let resp = run_adapter(scrapers_dir, source, &json!({ "op": op, "args": args })).await?;
    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("adapter {source} error: {err}"));
    }
    let recs = resp.get("records").cloned().unwrap_or(Value::Array(vec![]));
    Ok(serde_json::from_value(recs)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[tokio::test]
    async fn run_adapter_roundtrips_records() {
        let resp = run_adapter(&fixtures_dir(), "echo_adapter", &json!({"op": "x"}))
            .await.unwrap();
        assert_eq!(resp["records"][0]["title"], "Echo");
    }

    #[tokio::test]
    async fn fetch_papers_decodes_into_struct() {
        let papers = fetch_papers(&fixtures_dir(), "echo_adapter", "x", json!({}))
            .await.unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].title, "Echo");
        assert_eq!(papers[0].source, "HF");
    }

    #[tokio::test]
    async fn run_adapter_rejects_path_traversal_source() {
        let err = run_adapter(&fixtures_dir(), "../echo_adapter", &json!({}))
            .await.unwrap_err();
        assert!(err.to_string().contains("invalid adapter name"));
    }
}
```

- [ ] **Step 3: Register the module — edit `src/core/mod.rs`**

```rust
pub mod model;
pub mod scrape;
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib core::scrape`
Expected: PASS — both tests ok. (Requires `python3` on PATH.)

- [ ] **Step 5: Commit**

```bash
git add src/core/scrape.rs src/core/mod.rs tests/fixtures/echo_adapter.py
git commit -m "feat: scrape.rs single-shot stdio adapter runner + tests"
```

---

## Task 4: core/ai.rs — LAMU OpenAI-compat client

**Files:**
- Create: `src/core/ai.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Write the failing test in `src/core/ai.rs`**

```rust
//! AI summaries via LAMU's OpenAI-compat HTTP surface (spec §4.2).
//! One client; the `model` field selects local-vs-cloud routing inside LAMU.

use anyhow::{anyhow, Context, Result};
use serde_json::json;

pub struct AiClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl AiClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), model: model.into(), http: reqwest::Client::new() }
    }

    /// Mirrors the old `askClaude(prompt, data)` contract: an instruction plus
    /// an array of grounding strings -> one text answer.
    pub async fn summarize(&self, prompt: &str, items: &[String]) -> Result<String> {
        let content = format!("{prompt}\n\n{}", items.join("\n\n"));
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": content }],
            "temperature": 0.3,
            "max_tokens": 1200
        });
        let resp = self.http.post(&url).json(&body).send().await
            .context("POST to LAMU failed — is `lamu serve` running?")?;
        let status = resp.status();
        let val: serde_json::Value = resp.json().await.context("LAMU returned non-JSON")?;
        if !status.is_success() {
            return Err(anyhow!("LAMU {status}: {val}"));
        }
        val["choices"][0]["message"]["content"]
            .as_str().map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no choices[0].message.content in LAMU response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn summarize_posts_openai_shape_and_extracts_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "Summary text." } }]
            })))
            .mount(&server).await;

        let client = AiClient::new(server.uri(), "mimo-v2.5");
        let out = client.summarize("Summarize:", &["Paper A".into(), "Paper B".into()])
            .await.unwrap();
        assert_eq!(out, "Summary text.");
    }

    #[tokio::test]
    async fn summarize_errors_on_non_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "boom"})))
            .mount(&server).await;
        let client = AiClient::new(server.uri(), "mimo-v2.5");
        assert!(client.summarize("x", &[]).await.is_err());
    }
}
```

- [ ] **Step 2: Register the module — edit `src/core/mod.rs`**

```rust
pub mod model;
pub mod scrape;
pub mod ai;
```

- [ ] **Step 3: Run to verify it passes**

Run: `cargo test --lib core::ai`
Expected: PASS — both `summarize_*` tests ok.

- [ ] **Step 4: Commit**

```bash
git add src/core/ai.rs src/core/mod.rs
git commit -m "feat: ai.rs LAMU OpenAI-compat client + wiremock tests"
```

---

## Task 5: core/config.rs + core/feed.rs (HF-only orchestration)

**Files:**
- Create: `src/core/config.rs`, `src/core/feed.rs`, `tests/fixtures/feed/huggingface.py`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Write the failing test in `src/core/config.rs`**

```rust
//! Config + topic presets (spec §6). TOML at ~/.config/research/config.toml.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub label: String,
    pub hf: String,
    pub pubmed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub web_port: u16,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_lamu_url")]
    pub lamu_url: String,
    #[serde(default)]
    pub topic: Vec<Topic>,
}

fn default_port() -> u16 { 8787 }
fn default_model() -> String { "mimo-v2.5".into() }
fn default_lamu_url() -> String { "http://localhost:8020".into() }

impl Default for Config {
    fn default() -> Self {
        Config {
            web_port: default_port(),
            model: default_model(),
            lamu_url: default_lamu_url(),
            topic: eeg_preset(),
        }
    }
}

/// The shipped EEG/BCI preset (ported from the original artifact's TOPICS).
pub fn eeg_preset() -> Vec<Topic> {
    vec![
        Topic { id: "seizure".into(), label: "Seizure detection".into(),
            hf: "EEG seizure detection deep learning".into(),
            pubmed: "EEG seizure detection deep learning".into() },
        Topic { id: "foundation".into(), label: "Foundation models".into(),
            hf: "EEG foundation model pretraining transformer".into(),
            pubmed: "EEG foundation model machine learning".into() },
        Topic { id: "bci".into(), label: "BCI / motor imagery".into(),
            hf: "EEG motor imagery brain computer interface decoding".into(),
            pubmed: "EEG motor imagery brain computer interface".into() },
        Topic { id: "hardware".into(), label: "Hardware / AFE".into(),
            hf: "wearable EEG dry electrode hardware acquisition".into(),
            pubmed: "EEG wearable dry electrode amplifier acquisition device".into() },
    ]
}

impl Config {
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }
    /// If the user supplied no topics, fall back to the preset.
    pub fn topics(&self) -> Vec<Topic> {
        if self.topic.is_empty() { eeg_preset() } else { self.topic.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_eeg_preset_and_port_8787() {
        let c = Config::default();
        assert_eq!(c.web_port, 8787);
        assert_eq!(c.model, "mimo-v2.5");
        assert_eq!(c.topics().len(), 4);
        assert_eq!(c.topics()[0].id, "seizure");
    }

    #[test]
    fn parses_user_toml_and_overrides_topics() {
        let s = r#"
            web_port = 9000
            model = "deepseek-v4-pro"
            [[topic]]
            id = "nlp"
            label = "NLP"
            hf = "language models"
            pubmed = "language models"
        "#;
        let c = Config::from_toml(s).unwrap();
        assert_eq!(c.web_port, 9000);
        assert_eq!(c.model, "deepseek-v4-pro");
        assert_eq!(c.topics().len(), 1);
        assert_eq!(c.topics()[0].id, "nlp");
    }
}
```

- [ ] **Step 2: Write the failing test in `src/core/feed.rs`**

```rust
//! Orchestrate sources into one Feed. P0: HF papers only, per topic.

use crate::core::config::Topic;
use crate::core::model::{Feed, Paper, SourceError};
use crate::core::scrape::fetch_papers;
use serde_json::json;
use std::path::Path;

/// Load the feed for the given topics. Each topic failure is isolated into
/// `Feed.errors`; successful papers are merged, de-duped by title, newest first.
pub async fn load(scrapers_dir: &Path, topics: &[Topic], limit_per_topic: usize) -> Feed {
    let mut feed = Feed::default();
    for t in topics {
        let args = json!({ "query": t.hf, "limit": limit_per_topic, "topic": t.label });
        match fetch_papers(scrapers_dir, "huggingface", "paper_search", args).await {
            Ok(mut papers) => feed.papers.append(&mut papers),
            Err(e) => feed.errors.push(SourceError {
                source: format!("HF/{}", t.label),
                message: e.to_string(),
            }),
        }
    }
    dedup_by_title(&mut feed.papers);
    feed.papers.sort_by(|a, b| b.ts.cmp(&a.ts));
    feed
}

fn dedup_by_title(papers: &mut Vec<Paper>) {
    let mut seen = std::collections::HashSet::new();
    papers.retain(|p| {
        let k: String = p.title.to_lowercase().chars()
            .filter(|c| c.is_ascii_alphanumeric()).take(60).collect();
        // Empty-title papers (missing metadata) are kept, not silently dropped.
        k.is_empty() || seen.insert(k)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[tokio::test]
    async fn load_collects_papers_and_isolates_errors() {
        let topics = vec![Topic { id: "t".into(), label: "T".into(),
            hf: "q".into(), pubmed: "q".into() }];
        // tests/fixtures/feed/huggingface.py is the echo adapter (Step 3).
        let feed = load(&fixtures_dir().join("feed"), &topics, 5).await;
        assert_eq!(feed.errors.len(), 0);
        assert_eq!(feed.papers.len(), 1);
        assert_eq!(feed.papers[0].title, "Echo");
    }

    #[tokio::test]
    async fn missing_adapter_becomes_a_source_error() {
        let topics = vec![Topic { id: "t".into(), label: "T".into(),
            hf: "q".into(), pubmed: "q".into() }];
        let feed = load(Path::new("/nonexistent"), &topics, 5).await;
        assert_eq!(feed.papers.len(), 0);
        assert_eq!(feed.errors.len(), 1);
    }
}
```

- [ ] **Step 3: Create the feed fixture adapter `tests/fixtures/feed/huggingface.py`**

```python
#!/usr/bin/env python3
# Echo adapter aliased as "huggingface" for feed::load tests.
import json, sys
_ = sys.stdin.read()
sys.stdout.write(json.dumps({"records": [
    {"kind": "paper", "source": "HF", "topic": "T", "title": "Echo",
     "link": "https://example.com/x", "date_label": "2026-01-01",
     "ts": 1, "summary": "s", "grounding": "g"}
]}))
```

- [ ] **Step 4: Register modules — edit `src/core/mod.rs`**

```rust
pub mod model;
pub mod scrape;
pub mod ai;
pub mod config;
pub mod feed;
```

- [ ] **Step 5: Run to verify all pass**

Run: `cargo test --lib`
Expected: PASS — model, scrape, ai, config (2), feed (2).

- [ ] **Step 6: Commit**

```bash
git add src/core/config.rs src/core/feed.rs src/core/mod.rs tests/fixtures/feed/
git commit -m "feat: config preset + HF-only feed orchestration with error isolation"
```

---

## Task 6: server.rs — axum /api/feed + /api/summary + static

**Files:**
- Create: `src/server.rs`
- Modify: `src/lib.rs` (uncomment `pub mod server;`)

- [ ] **Step 1: Write the failing test in `src/server.rs`**

```rust
//! axum HTTP surface. Serves the built frontend and the JSON API.

use crate::core::ai::AiClient;
use crate::core::config::Topic;
use crate::core::feed;
use axum::{extract::State, routing::{get, post}, Json, Router};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub scrapers_dir: PathBuf,
    pub topics: Vec<Topic>,
    pub ai: Arc<AiClient>,
    pub dist_dir: PathBuf,
}

#[derive(Deserialize)]
pub struct SummaryReq {
    pub prompt: String,
    pub items: Vec<String>,
}

pub fn router(state: AppState) -> Router {
    let dist = state.dist_dir.clone();
    Router::new()
        .route("/api/feed", get(get_feed))
        .route("/api/summary", post(post_summary))
        .fallback_service(ServeDir::new(dist))
        .with_state(state)
}

async fn get_feed(State(s): State<AppState>) -> Json<crate::core::model::Feed> {
    Json(feed::load(&s.scrapers_dir, &s.topics, 8).await)
}

async fn post_summary(
    State(s): State<AppState>,
    Json(req): Json<SummaryReq>,
) -> Json<serde_json::Value> {
    match s.ai.summarize(&req.prompt, &req.items).await {
        Ok(text) => Json(serde_json::json!({ "text": text })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[tokio::test]
    async fn feed_endpoint_returns_papers_json() {
        let state = AppState {
            scrapers_dir: fixtures_dir().join("feed"),
            topics: vec![Topic { id: "t".into(), label: "T".into(),
                hf: "q".into(), pubmed: "q".into() }],
            ai: Arc::new(AiClient::new("http://127.0.0.1:1", "mimo-v2.5")),
            dist_dir: fixtures_dir(),
        };
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

        let body: serde_json::Value = reqwest::get(format!("http://{addr}/api/feed"))
            .await.unwrap().json().await.unwrap();
        assert_eq!(body["papers"][0]["title"], "Echo");
        assert_eq!(body["errors"].as_array().unwrap().len(), 0);
    }
}
```

- [ ] **Step 2: Enable the module — edit `src/lib.rs`**

```rust
pub mod core;
pub mod server;
```

- [ ] **Step 3: Run to verify it passes**

Run: `cargo test --lib server`
Expected: PASS — `feed_endpoint_returns_papers_json ... ok`.

- [ ] **Step 4: Commit**

```bash
git add src/server.rs src/lib.rs
git commit -m "feat: axum /api/feed + /api/summary + static serving"
```

---

## Task 7: Frontend (Vite + TS) — fetch feed, render cards (DOM-safe), summarize

> Security: the frontend never sets `innerHTML` from feed data. It builds nodes
> with `createElement` + `textContent`, and validates link schemes, so untrusted
> paper titles/URLs cannot inject script.

**Files:**
- Create: `frontend/package.json`, `frontend/tsconfig.json`, `frontend/vite.config.ts`, `frontend/index.html`
- Create: `frontend/src/types.ts`, `frontend/src/api.ts`, `frontend/src/render.ts`, `frontend/src/main.ts`, `frontend/src/render.test.ts`

- [ ] **Step 1: Create `frontend/package.json`**

```json
{
  "name": "research-frontend",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "typecheck": "tsc --noEmit",
    "test": "vitest run"
  },
  "devDependencies": {
    "typescript": "^5.5.0",
    "vite": "^5.4.0",
    "vitest": "^2.0.0",
    "jsdom": "^25.0.0"
  }
}
```

- [ ] **Step 2: Create `frontend/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "lib": ["ES2022", "DOM"],
    "types": ["vitest/globals"]
  },
  "include": ["src"]
}
```

- [ ] **Step 3: Create `frontend/vite.config.ts`**

```ts
import { defineConfig } from "vite";

export default defineConfig({
  build: { outDir: "dist" },
  server: { proxy: { "/api": "http://localhost:8787" } },
  test: { environment: "jsdom" },
});
```

- [ ] **Step 4: Create `frontend/src/types.ts`**

```ts
export interface Paper {
  kind: string;
  source: string;
  topic: string;
  title: string;
  link: string;
  date_label: string;
  ts: number;
  summary: string;
  grounding: string;
}

export interface SourceError { source: string; message: string; }
export interface Feed { papers: Paper[]; errors: SourceError[]; }
```

- [ ] **Step 5: Write the failing test `frontend/src/render.test.ts`**

```ts
import { describe, it, expect } from "vitest";
import { cardNode, safeHref } from "./render";
import type { Paper } from "./types";

const paper: Paper = {
  kind: "paper", source: "HF", topic: "Foundation models",
  title: "An <EEG> Model", link: "https://hf.co/papers/1",
  date_label: "2026-05-01", ts: 1, summary: "short", grounding: "g",
};

describe("render", () => {
  it("treats titles as text, not markup (no injection)", () => {
    const node = cardNode(paper);
    const a = node.querySelector("a")!;
    // textContent preserves the literal angle brackets; nothing is parsed as a tag
    expect(a.textContent).toBe("An <EEG> Model");
    expect(node.querySelector("EEG")).toBeNull();
  });
  it("renders source badge, topic, and link href", () => {
    const node = cardNode(paper);
    expect(node.querySelector(".badge")!.textContent).toBe("HF");
    expect(node.querySelector(".tlabel")!.textContent).toBe("Foundation models");
    expect(node.querySelector("a")!.getAttribute("href")).toBe("https://hf.co/papers/1");
  });
  it("rejects javascript: and data: scheme hrefs", () => {
    expect(safeHref("javascript:alert(1)")).toBe("#");
    expect(safeHref("data:text/html,<script>alert(1)</script>")).toBe("#");
    expect(safeHref("https://ok.com")).toBe("https://ok.com");
    expect(safeHref("http://ok.com")).toBe("http://ok.com");
  });
});
```

- [ ] **Step 6: Run to verify it fails**

Run: `cd frontend && npm install && npm test`
Expected: FAIL — cannot find `./render`.

- [ ] **Step 7: Implement `frontend/src/render.ts`**

```ts
import type { Paper, Feed } from "./types";

/** Allow only http(s) links; everything else collapses to a safe anchor. */
export function safeHref(url: string): string {
  return /^https?:\/\//i.test(url ?? "") ? url : "#";
}

function el(tag: string, className?: string, text?: string): HTMLElement {
  const n = document.createElement(tag);
  if (className) n.className = className;
  if (text != null) n.textContent = text;
  return n;
}

export function cardNode(p: Paper): HTMLElement {
  const card = el("div", "card");

  const titleWrap = el("div", "ctitle");
  const a = document.createElement("a");
  a.setAttribute("href", safeHref(p.link));
  a.target = "_blank";
  a.rel = "noopener";
  a.textContent = p.title;
  titleWrap.appendChild(a);
  card.appendChild(titleWrap);

  const meta = el("div", "meta");
  meta.appendChild(el("span", "badge", p.source));
  if (p.topic) meta.appendChild(el("span", "tlabel", p.topic));
  meta.appendChild(el("span", undefined, p.date_label || "—"));
  card.appendChild(meta);

  if (p.summary) card.appendChild(el("div", "summary", p.summary));
  return card;
}

export function renderFeed(container: HTMLElement, feed: Feed): void {
  container.replaceChildren();
  if (!feed.papers.length) {
    container.appendChild(el("div", "muted", "No papers found."));
    return;
  }
  for (const p of feed.papers) container.appendChild(cardNode(p));
}
```

- [ ] **Step 8: Run to verify it passes**

Run: `cd frontend && npm test`
Expected: PASS — 3 tests.

- [ ] **Step 9: Implement `frontend/src/api.ts`**

```ts
import type { Feed } from "./types";

export async function fetchFeed(): Promise<Feed> {
  const r = await fetch("/api/feed");
  if (!r.ok) throw new Error(`feed ${r.status}`);
  return r.json();
}

export async function summarize(prompt: string, items: string[]): Promise<string> {
  const r = await fetch("/api/summary", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, items }),
  });
  const j = await r.json();
  if (j.error) throw new Error(j.error);
  return j.text as string;
}
```

- [ ] **Step 10: Implement `frontend/src/main.ts`**

```ts
import { fetchFeed, summarize } from "./api";
import { renderFeed } from "./render";
import type { Paper } from "./types";

const PROMPT =
  "Synthesize the newest papers below into 2-3 short paragraphs: dominant themes, " +
  "anything new or surprising, and active directions. Ground claims only in the text. No preamble.";

function setMsg(target: HTMLElement, cls: string, text: string): void {
  target.replaceChildren();
  const d = document.createElement("div");
  d.className = cls;
  d.textContent = text;
  target.appendChild(d);
}

async function boot(): Promise<void> {
  const hero = document.getElementById("hero")!;
  const sumBody = document.getElementById("sumBody")!;
  setMsg(hero, "loading", "Loading papers…");
  try {
    const feed = await fetchFeed();
    renderFeed(hero, feed);
    document.getElementById("summarize")!.addEventListener("click", async () => {
      setMsg(sumBody, "loading", "Summarizing…");
      const items = feed.papers.slice(0, 14).map(
        (p: Paper) => `Title: ${p.title}\nAbstract: ${(p.grounding || p.summary).slice(0, 700)}`,
      );
      try {
        sumBody.textContent = await summarize(PROMPT, items);
      } catch (e) {
        setMsg(sumBody, "err", (e as Error).message);
      }
    });
  } catch (e) {
    setMsg(hero, "err", `Couldn't load feed: ${(e as Error).message}`);
  }
}
boot();
```

- [ ] **Step 11: Create `frontend/index.html`**

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>research</title>
  <style>
    body { font-family: -apple-system, system-ui, sans-serif; max-width: 1100px;
      margin: 0 auto; padding: 18px; background: #f7f8fa; color: #1a1d21; }
    .card { background: #fff; border: 1px solid #e6e8ec; border-radius: 10px;
      padding: 13px 14px; margin: 10px 0; }
    .badge { font-size: 10.5px; font-weight: 700; background: #fff4e5; color: #b5630c;
      padding: 2px 7px; border-radius: 5px; }
    .tlabel { background: #eef0f3; color: #5a6270; padding: 1px 7px; border-radius: 5px; }
    .meta { display: flex; gap: 8px; font-size: 11.5px; color: #828a96; margin-top: 6px; }
    .summary { font-size: 12.5px; color: #545b66; margin-top: 6px; }
    .loading, .muted { color: #9099a6; } .err { color: #b04a3a; }
    a { color: #2557d6; } button { cursor: pointer; }
  </style>
</head>
<body>
  <h1>research</h1>
  <button id="summarize">Summarize with AI</button>
  <div id="sumBody"></div>
  <h2>Newest papers</h2>
  <div id="hero"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 12: Typecheck + build**

Run: `cd frontend && npm run typecheck && npm run build`
Expected: PASS; `frontend/dist/` is produced.

- [ ] **Step 13: Commit**

```bash
printf 'node_modules/\ndist/\n' > frontend/.gitignore
git add frontend/
git commit -m "feat: vite+ts frontend — DOM-safe card render + AI summarize"
```

---

## Task 8: CLI wire-up + live end-to-end smoke (`research --check`)

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement `src/main.rs`**

```rust
use anyhow::Result;
use clap::Parser;
use research::core::ai::AiClient;
use research::core::config::Config;
use research::core::feed;
use research::server::{router, AppState};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "research", about = "Local research scraper / finder")]
struct Cli {
    /// Run a live end-to-end smoke check (1 fetch + 1 AI round-trip) and exit.
    #[arg(long)]
    check: bool,
    /// Directory holding the Python source adapters (default: bundled at build time).
    #[arg(long)]
    scrapers_dir: Option<PathBuf>,
    /// Directory holding the built frontend (default: bundled frontend/dist).
    #[arg(long)]
    dist_dir: Option<PathBuf>,
}

// NOTE: CARGO_MANIFEST_DIR is only the *default* — it bakes in the build-time
// source path, which is wrong after `cargo install` to another location. The
// `--scrapers-dir` / `--dist-dir` flags override it. P-subsequent "Install"
// replaces these defaults with an install-aware resolver.
fn default_scrapers_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scrapers")
}
fn default_dist_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::default();
    let ai = Arc::new(AiClient::new(cfg.lamu_url.clone(), cfg.model.clone()));
    let scrapers = cli.scrapers_dir.clone().unwrap_or_else(default_scrapers_dir);
    let dist = cli.dist_dir.clone().unwrap_or_else(default_dist_dir);

    if cli.check {
        let feed = feed::load(&scrapers, &cfg.topics()[..1], 3).await;
        println!("feed: {} papers, {} errors", feed.papers.len(), feed.errors.len());
        for e in &feed.errors { println!("  ERR {}: {}", e.source, e.message); }
        if let Some(p) = feed.papers.first() {
            match ai.summarize("One sentence on this paper:",
                &[format!("{}\n{}", p.title, p.grounding)]).await {
                Ok(txt) => println!("ai ok: {}", txt.lines().next().unwrap_or("")),
                Err(e) => println!("ai ERR (LAMU up?): {e}"),
            }
        }
        return Ok(());
    }

    let state = AppState {
        scrapers_dir: scrapers,
        topics: cfg.topics(),
        ai,
        dist_dir: dist,
    };
    let app = router(state);
    let addr = format!("127.0.0.1:{}", cfg.web_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("research web GUI on http://{addr}  (Ctrl-C to stop)");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Build the whole binary**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 3: Live smoke (requires `lamu serve` on :8020 + network)**

Run: `cargo run -- --check`
Expected output similar to:
```
feed: 3 papers, 0 errors
ai ok: <one-line summary>
```
If LAMU is down, the `feed` line still prints and the AI line shows `ai ERR (LAMU up?)` — acceptable for P0 (proves the feed path; rerun with LAMU up to prove AI).

- [ ] **Step 4: Manual web check**

Run: `cargo run`, then open `http://localhost:8787`. Expect: paper cards render; "Summarize with AI" fills the summary box (LAMU up).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: CLI entry + research --check live smoke + web serve"
```

---

## P0 Definition of Done

- `cargo test` — all green (model, scrape, ai, config, feed, server).
- `cd frontend && npm test && npm run build` — green; `dist/` produced.
- `cargo run -- --check` — prints papers count and (LAMU up) an AI line.
- `cargo run` + browser — cards render, AI summary works.

The spine (Rust ↔ Python stdio ↔ LAMU HTTP ↔ TS) is proven end-to-end.

---

## Subsequent phases (each gets its own plan via writing-plans)

Roadmap headers only — expand into full TDD task plans when P0 is green.

- **P1 — fan-out sources + cache + rate limits:** `pubmed.py`, `biorxiv.py`, `semantic.py`, HF repos/spaces ops in `huggingface.py`; `core/cache.rs` (disk TTL); `core/ratelimit.rs` (Rust-side per-source token bucket, spec §5.2); extend `feed.rs` to fan out concurrently (`futures::join_all`) and merge; extend frontend panels (repos, spaces, preprints→published).
- **P2 — UI parity + full TUI:** reading basket (backend-persisted), research summary block, deep-dive dropdown; `src/tui.rs` ratatui mirror (feed list, basket, summary, status/logs) + launcher keys (`w` opens browser, `r` reload, `q` quit); TUI calls `core` directly (no HTTP).
- **P3 — Google OAuth:** `gmail.py` + `drive.py` installed-app flow, `token.json` cache, read-only scopes; `/api/mail`, `/api/drive`; "Connect Google" placeholder when unconfigured.
- **P4 — optional semantic re-rank:** LAMU `/v1/embeddings` in `ai.rs`; rank/dedup papers by similarity behind a config flag.
- **Install:** `cargo install --path .` → `~/.cargo/bin/research`; replace `CARGO_MANIFEST_DIR` resource lookup with an install-aware resolver that bundles `scrapers/` + `frontend/dist/`.
```

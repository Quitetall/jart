# `research` — Local Research Scraper / Finder — Design

Date: 2026-06-04
Status: Approved (brainstorming) → ready for implementation plan
Owner: Brian Lam

## 1. Summary

A local, terminal-first research aggregator. Type `research` → a TUI pops up showing
freshly fetched papers, repos, preprints, and (optionally) Gmail/Drive research items,
with on-demand AI summaries. The same data is also served as a web GUI on a local port,
opened only when wanted. AI summarization runs through the local LAMU stack (default
`mimo-v2.5`), not a frontier model.

This is a re-platforming of an existing claude.ai artifact (`eeg-research-tracker.html`)
that depended on two runtime bridges only available inside claude.ai:
`window.cowork.callMcpTool(name, args)` and `window.cowork.askClaude(prompt, data)`.
Both are replaced by a local backend.

## 2. Goals

- `research` in a terminal opens a TUI with current research items (primary surface).
- Web GUI available on demand on a local port (`8787`), not auto-opened every launch.
- All source data fetched from **public APIs directly** (no claude.ai connectors).
- All AI summarization via **LAMU HTTP** (OpenAI-compat), default local `mimo-v2.5`.
- Topics are **user-configurable** via a config file; ships with an EEG/BCI preset.
- Resilient: one failing source does not break the rest; cached so reopening is cheap.

## 3. Non-goals (YAGNI for v1)

- No frontier-model summaries by default (cloud Claude is an opt-in model name only).
- No multi-user / hosted deployment. Single local user, localhost-bound.
- No write actions (no sending mail, no editing Drive). Read-only aggregation.
- Semantic re-ranking via embeddings is **deferred** to a later phase (P4), behind a flag.

## 4. Architecture

`research` is a single Rust binary (installed to `~/.cargo/bin/research`) that owns the
web server, the TUI, and all orchestration. Python provides stateless per-source scraper
adapters. LAMU's existing OpenAI-compat HTTP surface provides all AI.

```
research (rust bin)
 ├─ cli.rs       clap: `research` (TUI), `research web`, `research --check`, flags
 ├─ server.rs    axum: serve frontend/dist + JSON /api/*
 ├─ tui.rs       ratatui: feed mirror + status/logs + launcher keys (w = open web)
 ├─ core/
 │   ├─ feed.rs    orchestrate sources → normalized Feed (per-source Result)
 │   ├─ ai.rs      reqwest → LAMU :8020 /v1/chat/completions (model param)
 │   ├─ scrape.rs  spawn python adapter per request (stdio framing below)
 │   ├─ ratelimit.rs  per-source token bucket / pacing (Rust-side, persistent)
 │   ├─ cache.rs   disk cache, per-source TTL
 │   ├─ config.rs  load/merge topics + settings from config file
 │   └─ model.rs   Paper, Repo, Space, MailRow, DriveFile, Feed, SourceError
 └─ lamu.rs      ensure LAMU up (spawn `lamu serve` if :8020 down); teardown on exit

scrapers/ (python, uv venv)   each adapter: read {op,args} on stdin → JSON on stdout
 ├─ host nothing persistent — spawn-per-request, stateless
 ├─ huggingface.py   hf.co/api      papers, models, datasets, spaces
 ├─ pubmed.py        eutils.ncbi    search + metadata + full text (PMC)
 ├─ biorxiv.py       api.biorxiv.org  recent preprints + published
 ├─ semantic.py      api.semanticscholar.org   (replaces the "Consensus" panel)
 ├─ gmail.py         google oauth (installed-app) → token.json
 └─ drive.py         google oauth (shared token.json)

frontend/ (vite + ts)         vite build → dist/ (served by rust)
 ├─ index.html
 └─ src/  main.ts · api.ts (typed /api client) · render.ts · types.ts
```

### 4.0 Rust ↔ Python adapter protocol (stdio framing)

Each adapter is invoked **once per request** and is stateless. Framing is single-shot:

1. Rust spawns `python scrapers/<source>.py`.
2. Rust writes one JSON request object `{op, args}` to the adapter's **stdin**, then
   **closes stdin (EOF)**.
3. The adapter reads stdin **to EOF**, performs the HTTP call, writes **exactly one** JSON
   response object to **stdout**, and exits. `stderr` is captured for logs/diagnostics only.
4. Rust reads stdout to EOF, parses the single JSON object. Non-zero exit, unparseable
   stdout, or a per-request timeout → that source becomes a `SourceError` (others unaffected).

Because the adapter process does not persist, it holds **no cross-request state** — in
particular, no rate-limit token bucket (see §5.2). The adapter may apply a single in-call
429 backoff, but cross-request pacing is the Rust host's job.

### 4.1 Surfaces share one core

The TUI calls `core::feed` / `core::ai` functions **directly** (in-process). The web GUI
calls the same functions through axum `/api/*` handlers. There is exactly one feed-loading
and one AI path; the TUI and web GUI are two renderers over it.

### 4.2 AI surface (resolved from reading `lamu-rs`)

LAMU already serves an OpenAI-compat HTTP API (`lamu-api/src/openai_compat.rs`,
`lamu serve`, default `:8020`). Model routing is name-driven (`lamu-providers/cloud_config.rs`):
`mimo-v2.5` / `deepseek-*` / `qwen-*` route to local-or-cloud; `claude-opus-4-8` routes to
Anthropic `/v1/messages`. Therefore:

- Rust talks to LAMU with a **plain `reqwest` POST** to `/v1/chat/completions`. No
  hand-written MCP client.
- The "local vs cloud-Claude" choice is **just the model name** — one client, one env var.
  `RESEARCH_MODEL` (default `mimo-v2.5`). Cloud Claude requires `ANTHROPIC_API_KEY` in
  LAMU's environment (already present for the reviewer ensemble).
- `ai.rs` exposes `summarize(prompt: &str, items: &[String]) -> String`, mirroring the old
  `askClaude(prompt, data)` contract.

If LAMU is not already listening on `:8020`, `research` spawns `lamu serve` as a managed
child and tears it down on exit. `--lamu-url` / `--lamu-port` override; if a LAMU is
already up, `research` uses it and does not manage its lifecycle.

## 5. Sources, rate limits, caching

Each source is an independent Python adapter invoked per request. Adapters return
**normalized JSON** matching `core::model` shapes, so the frontend never parses markdown
(removing all the brittle regex parsing from the original artifact).

| Source | Endpoint | Notes / limits |
|--------|----------|----------------|
| HF papers | `hf.co/api/...` | optional `HF_TOKEN` for higher limits |
| HF repos/spaces | `hf.co/api/models|datasets|spaces` | trending/likes sort |
| PubMed | `eutils.ncbi.nlm.nih.gov` | **3 req/s without key, 10 with** `NCBI_API_KEY`; full text via PMC |
| bioRxiv/medRxiv | `api.biorxiv.org` | recent window + published-version lookup |
| Semantic Scholar | `api.semanticscholar.org` | throttles hard; `S2_API_KEY` recommended |
| Gmail | Gmail API | google installed-app OAuth, read-only scope |
| Drive | Drive API | shared OAuth token, read-only scope |

### 5.1 Caching (first-class)

`cache.rs` stores normalized per-source results on disk
(`~/.cache/research/<source>/<query-hash>.json`) with a per-source TTL (e.g. papers 6h,
repos 12h, mail 1h). `feed.rs` serves cache on hit; a manual reload (TUI `r`, web Reload,
or `--no-cache`) forces a refetch. This prevents re-hitting public APIs on every open.

### 5.2 Rate-limit handling

Cross-request pacing lives in the **Rust host** (`core/ratelimit.rs`), not in the adapters —
a spawn-per-request Python process is stateless and cannot hold a token bucket across
invocations. `ratelimit.rs` keeps one persistent token bucket per source (e.g. PubMed 3/s,
or 10/s when `NCBI_API_KEY` is set) and `feed.rs` acquires a permit before dispatching each
`scrape.rs` call. Each adapter exposes only its source's rate-limit *config* (requests/sec,
whether a key raises it) so the Rust limiter can be configured correctly; an adapter may
additionally apply a single in-call exponential backoff on a 429, but does no cross-request
pacing.

Missing optional API keys degrade gracefully (lower bucket rate, or the source returns an
empty result with a `SourceError` note rather than failing the feed).

## 6. Configuration

A single config file at `~/.config/research/config.toml` (override `--config`):

```toml
web_port = 8787
model = "mimo-v2.5"          # any LAMU-routable model name
lamu_url = "http://localhost:8020"
default_topics = ["seizure", "foundation", "bci", "hardware"]

[[topic]]
id = "seizure"
label = "Seizure detection"
hf = "EEG seizure detection deep learning"
pubmed = "EEG seizure detection deep learning"
# ... user adds/edits topics freely
```

Ships with the EEG/BCI preset (the four topics from the original artifact). Topics drive
HF + PubMed + Semantic Scholar queries and preprint classification. Enabled/disabled topic
state persists (config or a small state file), replacing the original `localStorage` chips.

API keys come from env (`NCBI_API_KEY`, `S2_API_KEY`, `HF_TOKEN`) — never written to config.

## 7. Data flow

1. `research` boots → load config → ensure LAMU (`:8020`) up → start axum on `web_port` →
   draw TUI. **Browser is not opened automatically.**
2. TUI requests feed → `core::feed::load(topics)` → `cache.rs` (hit returns immediately) →
   on miss, fan out `scrape.rs` calls (one Python proc per source, concurrent) →
   normalize → cache → return `Feed` with per-source `Result`s.
3. AI summary (TUI key or web button) → `core::ai::summarize(prompt, items)` → LAMU `:8020`
   → text. Same path for deep-dive and basket summary.
4. Web GUI: `api.ts` calls `GET /api/feed`, `POST /api/summary`, etc.; axum handlers call
   the same `core` functions. `w` in the TUI (or `research web`) opens the browser to
   `http://localhost:<web_port>`.
5. Quit (`q` / Ctrl-C): kill any in-flight Python procs, tear down managed LAMU, stop axum.

## 8. Error handling

- Per-source isolation: `feed.rs` uses `join_all` over `Result<SourceData, SourceError>`;
  a failing source yields an error marker the UI shows as "Couldn't load X", matching the
  original's per-panel try/catch behavior.
- LAMU unreachable: AI calls surface a clear "AI unavailable — is LAMU running?" message;
  feed still loads (AI is independent of fetching).
- OAuth not configured: Gmail/Drive panels show a "Connect Google" prompt instead of
  erroring the whole feed.

## 9. Testing

- **Python adapters:** unit tests against recorded HTTP fixtures (no live calls in CI);
  one opt-in live smoke test per source behind an env flag.
- **Rust core:** `scrape.rs` against a fake adapter binary; `ai.rs` against a stub LAMU
  (local HTTP mock); `feed.rs` orchestration with mocked scrapers (incl. partial failure);
  `cache.rs` TTL hit/miss/expiry.
- **Frontend:** `tsc` type-check + a light render test of `render.ts` over fixture feeds.
- **End-to-end:** `research --check` performs one live fetch per source + one AI round-trip
  and prints a pass/fail table.

## 10. Build order — walking skeleton (riskiest integration first)

- **P0 — spine:** HF-papers adapter → stdio → `scrape.rs` → axum `/api/feed` → one card in
  the TS frontend + one `/api/summary` via LAMU. Proves Rust↔Python↔LAMU↔TS end-to-end.
  Plus `lamu.rs` ensure-up and basic `config.rs`.
- **P1 — fan-out:** remaining public sources (PubMed, bioRxiv + published, Semantic Scholar,
  HF repos/spaces) + `cache.rs` + rate limits.
- **P2 — UI parity:** reading basket, research summary, deep-dive, and the **full TUI mirror**.
- **P3 — Google:** Gmail + Drive installed-app OAuth, `token.json` cache, read-only scopes.
- **P4 — optional:** semantic re-rank via LAMU `/v1/embeddings`, behind a config flag.

## 11. Project layout

```
~/Desktop/research/
  Cargo.toml                      # single bin crate `research`
  src/                            # cli, server, tui, core/, lamu
  scrapers/                       # python adapters + pyproject (uv)
  frontend/                       # vite + ts, builds to frontend/dist
  config.example.toml
  docs/superpowers/specs/2026-06-04-research-tool-design.md
  docs/decisions/                 # ADRs (per user convention)
```

## 12. Open questions

None blocking. Cloud-Claude summaries remain available purely by setting
`model = "claude-opus-4-8"`; no extra code path. Embedding-based re-rank (P4) is the only
explicitly deferred feature.

# jart — Just Another Research Tool

A local, terminal-first research aggregator. Type `jart` and a TUI pops up with the
newest papers, preprints, models, and datasets for the topics you care about — pulled
live from public APIs, summarized by a local LLM, with a web GUI on the side when you
want it. No cloud account, no data leaving your machine.

Built for tracking EEG/BCI research, but the topics are fully configurable for any field.

```
$ jart            # TUI (primary surface)
$ jart --web      # serve + open the web GUI instead
$ jart --check    # one live fetch + AI round-trip, then exit
```

`research` is an installed alias for `jart` — same tool, either name.

## Sources

| Source | What |
|--------|------|
| Hugging Face | papers, trending models & datasets, Spaces |
| PubMed | E-utilities (esearch + efetch) |
| bioRxiv / medRxiv | recent preprints + preprint→published |
| Semantic Scholar | graph paper search |

All fetched directly from public APIs — concurrent fan-out, disk-cached (papers 6h,
repos 12h), with Rust-side per-source rate limiting and per-source error isolation
(one source throttling never breaks the feed).

## Architecture

A single Rust binary owns the web server (axum), the TUI (ratatui), and orchestration.
Stateless Python adapters fetch each source over a single-shot stdio JSON protocol. AI
summaries go to a local [LAMU](https://github.com/) OpenAI-compatible endpoint. The
frontend is a Vite + TypeScript SPA (DOM-safe, no `innerHTML`).

```
jart (rust)
 ├─ cli            jart / research entrypoints
 ├─ server         axum: serves frontend/dist + /api/feed, /api/summary
 ├─ tui            ratatui: feed list, AI summary, open-in-browser
 └─ core
     ├─ feed       concurrent join_all over (topic × source) + repos/spaces
     ├─ scrape     spawn a Python adapter, single-shot stdio, timeout + reap
     ├─ cache      disk cache, per-source TTL
     ├─ ratelimit  per-source min-interval pacer
     ├─ ai         LAMU /v1/chat/completions client
     └─ config     topics + settings (TOML)
scrapers/   huggingface.py · pubmed.py · biorxiv.py · semantic.py  (Python, stdlib only)
frontend/   Vite + TypeScript SPA
```

## Install

Requires a recent Rust toolchain, Python 3, and Node (to build the frontend once).

```bash
cd jart
(cd frontend && npm install && npm run build)   # build the web UI -> frontend/dist
cargo install --path .                            # installs `jart` + `research`
```

The installed binary resolves its bundled `scrapers/` and `frontend/dist/` from this
repo directory, so keep the repo in place (an install-aware resolver is planned).

AI summaries require a local LAMU server on `:8020` serving a local chat model
(`lamu serve`). The feed works without it; only summaries need it.

## Usage

**TUI** (`jart`): `↑/↓` (or `j/k`) move · `Enter` open paper · `w` open web GUI ·
`s` summarize · `r` reload · `Esc` clear summary · `q` / `Ctrl-C` quit.

**Web** (`jart --web`, or `w` from the TUI): serves the SPA at `http://localhost:8787`.

### Config

`~/.config/jart/config.toml` (override with `--config`):

```toml
web_port = 8787
model = "qwen3.6-27b-uncensored-heretic-v2-q4_k_m"   # any local id from :8020/v1/models
lamu_url = "http://localhost:8020"

[[topic]]
id = "seizure"
label = "Seizure detection"
hf = "EEG seizure detection deep learning"   # first token is the bioRxiv relevance anchor
pubmed = "EEG seizure detection deep learning"
```

Ships with an EEG/BCI topic preset. Topics drive the HF / PubMed / Semantic queries.

### API keys (recommended)

Set these in your environment for faster, more reliable fetches (both degrade
gracefully without them):

```bash
export NCBI_API_KEY=…   # PubMed 3→10 req/s (removes cold-load 429s)
export S2_API_KEY=…     # Semantic Scholar (heavily throttled unkeyed)
export HF_TOKEN=…       # optional, higher Hugging Face limits
```

Keys are read from the environment only — never written to config or cache.

## Status

- **P0** — walking skeleton: TUI + web + HF papers + local-LLM summaries.
- **P1** — sources (PubMed, bioRxiv/medRxiv, Semantic Scholar, HF repos/spaces),
  disk cache, rate limiting, concurrent fan-out. *(current)*
- **P2** (planned) — save/persist papers, AI search/rank → reading basket, richer TUI
  (abstract pane, source filter, repos/spaces section, basket), web panels.

Design + plans live in `docs/superpowers/`.

## License

TBD.

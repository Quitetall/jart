//! Orchestrate sources into one Feed. P1: concurrent fan-out over
//! (topic × paper-source) plus a one-shot HF repos/spaces fetch, each paced by
//! the host-side `Pacer` and memoized on disk by the `Cache`.
//!
//! Error isolation is preserved: a per-task failure becomes a `SourceError`
//! while every other task still contributes its results.

use crate::core::cache::Cache;
use crate::core::config::Topic;
use crate::core::model::{Feed, Paper, Repo, Space, SourceError};
use crate::core::ratelimit::Pacer;
use crate::core::scrape::fetch_records;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// Papers are cheap to refetch but volatile; 6h keeps the feed fresh.
const PAPER_TTL: Duration = Duration::from_secs(6 * 3600);
/// Repos/spaces (trending models, popular spaces) move slowly; 12h.
const REPO_TTL: Duration = Duration::from_secs(12 * 3600);

/// One enabled paper source: (adapter-name, op, label-prefix).
/// `label_prefix` is what the UI badges show and what errors are tagged with.
struct PaperSource {
    adapter: &'static str,
    op: &'static str,
    label: &'static str,
}

const PAPER_SOURCES: &[PaperSource] = &[
    PaperSource { adapter: "huggingface", op: "paper_search", label: "HF" },
    PaperSource { adapter: "pubmed", op: "search", label: "PubMed" },
    PaperSource { adapter: "biorxiv", op: "search", label: "Preprint" },
    PaperSource { adapter: "semantic", op: "search", label: "Semantic" },
    // Web search (Serper/Tavily). Opt-in: the adapter returns empty without an API
    // key, so it adds general-web results when SERPER_API_KEY/TAVILY_API_KEY is set
    // and is a silent no-op otherwise.
    PaperSource { adapter: "web", op: "search", label: "Web" },
];

/// Outcome of one fan-out task. Kept as an enum (not a bare Result) so the
/// merge step can route papers/repos/spaces/errors to the right Feed field.
enum TaskOut {
    Papers(Vec<Paper>),
    Repos(Vec<Repo>),
    Spaces(Vec<Space>),
    Err(SourceError),
}

/// Cache key for a paper task: source + op + query + limit (all that varies a
/// result). Stable across runs so a second load is a hit.
fn paper_key(op: &str, query: &str, limit: usize) -> String {
    format!("{op}|{query}|{limit}")
}

/// Cache-first typed fetch: on a fresh hit, decode the cached `records` array;
/// otherwise run the adapter, then write the decoded records back to cache.
/// Returns the typed records or an error (adapter failure / undecodable cache).
async fn cached_fetch<T>(
    scrapers_dir: &Path,
    cache: &Cache,
    cache_source: &str,
    key: &str,
    ttl: Duration,
    adapter: &str,
    op: &str,
    args: Value,
) -> anyhow::Result<Vec<T>>
where
    T: DeserializeOwned + Serialize,
{
    if let Some(cached) = cache.get(cache_source, key, ttl) {
        // A corrupt/stale-shape cache entry shouldn't poison the load: on a
        // decode failure, fall through to a live fetch instead of erroring.
        if let Ok(recs) = serde_json::from_value::<Vec<T>>(cached) {
            return Ok(recs);
        }
    }
    let recs: Vec<T> = fetch_records(scrapers_dir, adapter, op, args).await?;
    // Best-effort memoize. Re-serialize the typed records (not the raw adapter
    // envelope) so the cache stores exactly what we decode on a hit.
    if let Ok(val) = serde_json::to_value(&recs) {
        cache.put(cache_source, key, &val);
    }
    Ok(recs)
}

/// Load the feed for the given topics, fanning out concurrently.
///
/// Tasks: one per (topic × paper-source), plus one HF `repo_search` and one
/// `space_search` (topic-independent). Each task paces itself via `pacer` and
/// consults `cache` before hitting the network. Per-task errors are isolated
/// into `Feed.errors`; papers are merged, de-duped by title, newest first.
pub async fn load(
    scrapers_dir: &Path,
    topics: &[Topic],
    limit_per_topic: usize,
    cache: &Cache,
    pacer: &Pacer,
) -> Feed {
    let mut tasks: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = TaskOut> + Send>>> =
        Vec::new();

    // Paper tasks: topic × source.
    for t in topics {
        for src in PAPER_SOURCES {
            // HF searches use the HF-tuned query; the literature DBs share the
            // pubmed query string.
            let query = if src.adapter == "huggingface" { t.hf.clone() } else { t.pubmed.clone() };
            let label = format!("{}/{}", src.label, t.label);
            let args = json!({ "query": query, "limit": limit_per_topic, "topic": t.label });
            let key = paper_key(src.op, &query, limit_per_topic);
            tasks.push(Box::pin(paper_task(
                scrapers_dir, cache, pacer, src.adapter, src.op, args, key, label,
            )));
        }
    }

    // One-shot repos + spaces (not per topic). HF repo/space search matches
    // short repo NAMES, so a verbose topic query ("EEG seizure detection deep
    // learning") returns nothing — use the first token ("EEG") instead.
    if let Some(first) = topics.first() {
        let q = first
            .hf
            .split_whitespace()
            .next()
            .unwrap_or(first.hf.as_str())
            .to_string();
        let r_args = json!({
            "query": q, "repo_types": ["model", "dataset"],
            "sort": "trendingScore", "limit": limit_per_topic,
        });
        tasks.push(Box::pin(repo_task(scrapers_dir, cache, pacer, q.clone(), r_args, limit_per_topic)));
        let s_args = json!({ "query": q, "sort": "likes", "limit": limit_per_topic });
        tasks.push(Box::pin(space_task(scrapers_dir, cache, pacer, q, s_args, limit_per_topic)));
    }

    let outs = futures::future::join_all(tasks).await;

    let mut feed = Feed::default();
    for out in outs {
        match out {
            TaskOut::Papers(mut p) => feed.papers.append(&mut p),
            TaskOut::Repos(mut r) => feed.repos.append(&mut r),
            TaskOut::Spaces(mut s) => feed.spaces.append(&mut s),
            TaskOut::Err(e) => feed.errors.push(e),
        }
    }

    dedup_by_title(&mut feed.papers);
    feed.papers.sort_by(|a, b| b.ts.cmp(&a.ts));
    feed
}

async fn paper_task(
    scrapers_dir: &Path,
    cache: &Cache,
    pacer: &Pacer,
    adapter: &str,
    op: &str,
    args: Value,
    key: String,
    label: String,
) -> TaskOut {
    pacer.acquire(adapter).await;
    match cached_fetch::<Paper>(scrapers_dir, cache, adapter, &key, PAPER_TTL, adapter, op, args)
        .await
    {
        Ok(papers) => TaskOut::Papers(papers),
        Err(e) => TaskOut::Err(SourceError { source: label, message: short_err(&e) }),
    }
}

async fn repo_task(
    scrapers_dir: &Path,
    cache: &Cache,
    pacer: &Pacer,
    query: String,
    args: Value,
    limit: usize,
) -> TaskOut {
    pacer.acquire("huggingface").await;
    let key = format!("repo_search|{query}|{limit}");
    match cached_fetch::<Repo>(
        scrapers_dir, cache, "huggingface", &key, REPO_TTL, "huggingface", "repo_search", args,
    )
    .await
    {
        Ok(repos) => TaskOut::Repos(repos),
        Err(e) => TaskOut::Err(SourceError { source: "HF/repos".into(), message: short_err(&e) }),
    }
}

async fn space_task(
    scrapers_dir: &Path,
    cache: &Cache,
    pacer: &Pacer,
    query: String,
    args: Value,
    limit: usize,
) -> TaskOut {
    pacer.acquire("huggingface").await;
    let key = format!("space_search|{query}|{limit}");
    match cached_fetch::<Space>(
        scrapers_dir, cache, "huggingface", &key, REPO_TTL, "huggingface", "space_search", args,
    )
    .await
    {
        Ok(spaces) => TaskOut::Spaces(spaces),
        Err(e) => TaskOut::Err(SourceError { source: "HF/spaces".into(), message: short_err(&e) }),
    }
}

/// First line of an error, capped — an adapter's full stderr traceback must not
/// bloat the serialized Feed (the message is a `Serialize` field shown in the UI).
fn short_err(e: &anyhow::Error) -> String {
    e.to_string().lines().next().unwrap_or("error").chars().take(500).collect()
}

fn dedup_by_title(papers: &mut Vec<Paper>) {
    let mut seen = std::collections::HashSet::new();
    papers.retain(|p| {
        // Normalized title key, capped at 200 chars: 60 was too short (collided
        // "…v1"/"…v2"); fully unbounded lets a pathological title bloat the set.
        let k: String = p.title.to_lowercase().chars()
            .filter(|c| c.is_ascii_alphanumeric()).take(200).collect();
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

    /// A unique temp cache root per test so parallel runs never collide.
    fn temp_cache(tag: &str) -> (Cache, PathBuf) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut h);
        tag.hash(&mut h);
        let root = std::env::temp_dir().join(format!("research_feed_cache_{:016x}", h.finish()));
        (Cache::with_root(&root), root)
    }

    fn topic(label: &str) -> Topic {
        Topic { id: "t".into(), label: label.into(), hf: "q".into(), pubmed: "q".into() }
    }

    #[tokio::test]
    async fn load_collects_papers_and_isolates_errors() {
        // tests/fixtures/feed/ has only huggingface.py (the echo adapter); the
        // other paper sources (pubmed/biorxiv/semantic) are absent -> they each
        // become a SourceError, while HF papers still load. This is the
        // partial-failure isolation guarantee.
        let topics = vec![topic("T")];
        let (cache, root) = temp_cache("collect");
        let pacer = Pacer::new();
        let feed = load(&fixtures_dir().join("feed"), &topics, 5, &cache, &pacer).await;
        assert_eq!(feed.papers.len(), 1, "HF echo paper must load");
        assert_eq!(feed.papers[0].title, "Echo");
        // pubmed + biorxiv + semantic adapters missing -> 3 paper errors, plus
        // HF repo_search/space_search ops are absent on the echo adapter -> 2
        // more. The HF paper source itself succeeds.
        assert!(feed.errors.len() >= 3, "missing sources must isolate as errors: {:?}", feed.errors);
        assert!(feed.errors.iter().all(|e| !e.source.starts_with("HF/T")),
            "the HF paper source must not error");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn second_load_is_served_from_cache() {
        // First load populates the cache; deleting the adapter dir afterwards
        // proves the second load is served entirely from disk (no adapter run).
        let topics = vec![topic("T")];
        let (cache, root) = temp_cache("cachehit");
        let pacer = Pacer::new();

        let feed1 = load(&fixtures_dir().join("feed"), &topics, 5, &cache, &pacer).await;
        assert_eq!(feed1.papers.len(), 1);

        // Point at a nonexistent dir: a live fetch would now fail, so a surviving
        // HF paper can only come from the cache written on the first load.
        let feed2 = load(Path::new("/nonexistent"), &topics, 5, &cache, &pacer).await;
        assert_eq!(feed2.papers.len(), 1, "HF paper must be served from cache");
        assert_eq!(feed2.papers[0].title, "Echo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn all_sources_missing_yields_only_errors() {
        let topics = vec![topic("T")];
        let (cache, root) = temp_cache("allmissing");
        let pacer = Pacer::new();
        let feed = load(Path::new("/nonexistent"), &topics, 5, &cache, &pacer).await;
        assert_eq!(feed.papers.len(), 0);
        // 5 paper sources (HF, PubMed, Preprint, Semantic, Web) + repo + space = 7
        // tasks, all failing.
        assert_eq!(feed.errors.len(), 7);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn repos_and_spaces_load_from_adapter() {
        // tests/fixtures/hub/huggingface.py answers repo_search + space_search.
        let topics = vec![topic("T")];
        let (cache, root) = temp_cache("hub");
        let pacer = Pacer::new();
        let feed = load(&fixtures_dir().join("hub"), &topics, 5, &cache, &pacer).await;
        assert_eq!(feed.repos.len(), 1, "repo_search must populate feed.repos");
        assert_eq!(feed.repos[0].name, "org/eeg-net");
        assert_eq!(feed.spaces.len(), 1, "space_search must populate feed.spaces");
        assert_eq!(feed.spaces[0].name, "org/eeg-demo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn two_sources_merge_papers() {
        // tests/fixtures/multi/ provides huggingface.py + pubmed.py, each
        // emitting a distinct paper; the feed merges both.
        let topics = vec![topic("T")];
        let (cache, root) = temp_cache("multi");
        let pacer = Pacer::new();
        let feed = load(&fixtures_dir().join("multi"), &topics, 5, &cache, &pacer).await;
        let titles: Vec<&str> = feed.papers.iter().map(|p| p.title.as_str()).collect();
        assert!(titles.contains(&"HF Paper"), "HF source merged: {titles:?}");
        assert!(titles.contains(&"PubMed Paper"), "PubMed source merged: {titles:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

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

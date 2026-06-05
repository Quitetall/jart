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

/// A Hugging Face model or dataset repo. JSON field names are the wire contract
/// with the `huggingface.py` adapter (`repo_search` op) and the TS frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repo {
    pub kind: String,        // "model" | "dataset"
    pub name: String,
    pub link: String,
    pub downloads: String,
    pub likes: String,
}

/// A Hugging Face Space. Wire contract with the `space_search` op.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Space {
    pub name: String,
    pub link: String,
    pub likes: String,
    pub sdk: String,
}

/// A source that failed to load. Surfaced per-panel in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceError {
    pub source: String,
    pub message: String,
}

/// Aggregated result of one feed load.
///
/// `repos`/`spaces` are additive (P1): `#[serde(default)]` keeps older payloads
/// without those keys deserializable, and the `Default` derive still holds so
/// `Feed::default()` / `..Default::default()` callers keep working.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feed {
    pub papers: Vec<Paper>,
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default)]
    pub spaces: Vec<Space>,
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

    #[test]
    fn repo_and_space_json_roundtrip() {
        let r = Repo {
            kind: "model".into(),
            name: "org/eeg-net".into(),
            link: "https://huggingface.co/models/org/eeg-net".into(),
            downloads: "1234".into(),
            likes: "56".into(),
        };
        let back: Repo = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);

        let s = Space {
            name: "org/eeg-demo".into(),
            link: "https://huggingface.co/spaces/org/eeg-demo".into(),
            likes: "7".into(),
            sdk: "gradio".into(),
        };
        let back: Space = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn feed_repos_and_spaces_default_when_absent() {
        // An older payload (P0) without repos/spaces must still deserialize.
        let json = r#"{"papers":[],"errors":[]}"#;
        let feed: Feed = serde_json::from_str(json).unwrap();
        assert!(feed.repos.is_empty());
        assert!(feed.spaces.is_empty());
    }
}

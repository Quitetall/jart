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

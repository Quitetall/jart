//! axum HTTP surface. Serves the built frontend and the JSON API.

use crate::core::ai::AiClient;
use crate::core::cache::Cache;
use crate::core::config::Topic;
use crate::core::feed;
use crate::core::ratelimit::Pacer;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
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
    pub cache: Arc<Cache>,
    pub pacer: Arc<Pacer>,
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
    Json(feed::load(&s.scrapers_dir, &s.topics, 8, &s.cache, &s.pacer).await)
}

async fn post_summary(
    State(s): State<AppState>,
    Json(req): Json<SummaryReq>,
) -> impl IntoResponse {
    match s.ai.summarize(&req.prompt, &req.items).await {
        Ok(text) => (StatusCode::OK, Json(serde_json::json!({ "text": text }))),
        // 502: the failure is upstream (LAMU), not the client's request.
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "error": e.to_string() }))),
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
        let cache_root = std::env::temp_dir().join("research_server_test_cache");
        let _ = std::fs::remove_dir_all(&cache_root);
        let state = AppState {
            scrapers_dir: fixtures_dir().join("feed"),
            topics: vec![Topic { id: "t".into(), label: "T".into(),
                hf: "q".into(), pubmed: "q".into() }],
            ai: Arc::new(AiClient::new("http://127.0.0.1:1", "mimo-v2.5")),
            dist_dir: fixtures_dir(),
            cache: Arc::new(Cache::with_root(&cache_root)),
            pacer: Arc::new(Pacer::new()),
        };
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

        let body: serde_json::Value = reqwest::get(format!("http://{addr}/api/feed"))
            .await.unwrap().json().await.unwrap();
        // The HF echo paper still loads end-to-end over the HTTP surface.
        assert_eq!(body["papers"][0]["title"], "Echo");
        // P1 feed shape: repos/spaces are always present (here empty — the echo
        // fixture only answers paper_search). The other paper sources
        // (pubmed/biorxiv/semantic) are absent from this fixture dir, so they
        // isolate as source errors while the HF papers still serve.
        assert!(body["repos"].is_array(), "feed JSON must carry a repos array");
        assert!(body["spaces"].is_array(), "feed JSON must carry a spaces array");
        let _ = std::fs::remove_dir_all(&cache_root);
    }
}

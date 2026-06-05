//! Spawn a Python adapter and exchange one JSON message over stdio.
//! Framing (spec §4.0): write request to stdin, close stdin, read stdout to EOF.

use crate::core::model::Paper;
use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

/// Per-adapter wall-clock budget. Spec §4.0: timeout -> SourceError.
const ADAPTER_TIMEOUT: Duration = Duration::from_secs(30);

/// Run `<scrapers_dir>/<source>.py`, sending `request`, returning parsed JSON.
/// Bounded by `ADAPTER_TIMEOUT`.
pub async fn run_adapter(scrapers_dir: &Path, source: &str, request: &Value) -> Result<Value> {
    run_adapter_to(scrapers_dir, source, request, ADAPTER_TIMEOUT).await
}

/// Same as `run_adapter` but with an explicit budget (used by tests to avoid a
/// 30s wait against a hung adapter).
async fn run_adapter_to(
    scrapers_dir: &Path,
    source: &str,
    request: &Value,
    budget: Duration,
) -> Result<Value> {
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
        .kill_on_drop(true) // a timed-out adapter is reaped, not orphaned
        .spawn()
        .with_context(|| format!("spawn adapter {}", script.display()))?;

    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    stdin.write_all(request.to_string().as_bytes()).await?;
    drop(stdin); // close -> EOF so the adapter's stdin.read() returns

    // On elapse, the wait_with_output future (owning `child`) is dropped;
    // kill_on_drop(true) then kills the subprocess. -> SourceError upstream.
    let out = match timeout(budget, child.wait_with_output()).await {
        Ok(res) => res?,
        Err(_) => return Err(anyhow!("adapter {source} timed out after {budget:?}")),
    };
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

/// Generic: run an adapter op and decode its `records` array into `Vec<T>`.
///
/// Reuses `run_adapter` (and thus its `ADAPTER_TIMEOUT` + `kill_on_drop`
/// reaping). An adapter that returns an `{error}` envelope (its graceful
/// failure mode, spec §4.0) becomes an `Err` here so the caller can surface a
/// `SourceError` instead of crashing.
pub async fn fetch_records<T: DeserializeOwned>(
    scrapers_dir: &Path,
    source: &str,
    op: &str,
    args: Value,
) -> Result<Vec<T>> {
    let resp = run_adapter(scrapers_dir, source, &json!({ "op": op, "args": args })).await?;
    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("adapter {source} error: {err}"));
    }
    let recs = resp.get("records").cloned().unwrap_or(Value::Array(vec![]));
    serde_json::from_value(recs)
        .with_context(|| format!("adapter {source}/{op} returned undecodable records"))
}

/// Convenience: run an adapter op and decode `records` into `Vec<Paper>`.
pub async fn fetch_papers(
    scrapers_dir: &Path,
    source: &str,
    op: &str,
    args: Value,
) -> Result<Vec<Paper>> {
    fetch_records::<Paper>(scrapers_dir, source, op, args).await
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
    async fn fetch_records_decodes_generic_type() {
        // The repo_echo fixture emits a Repo-shaped record; fetch_records::<Repo>
        // proves the generic decode is not Paper-bound.
        use crate::core::model::Repo;
        let repos: Vec<Repo> =
            fetch_records(&fixtures_dir(), "repo_echo", "repo_search", json!({}))
                .await
                .unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].kind, "model");
        assert_eq!(repos[0].name, "org/eeg-net");
    }

    #[tokio::test]
    async fn run_adapter_rejects_path_traversal_source() {
        let err = run_adapter(&fixtures_dir(), "../echo_adapter", &json!({}))
            .await.unwrap_err();
        assert!(err.to_string().contains("invalid adapter name"));
    }

    #[tokio::test]
    async fn run_adapter_times_out_on_hung_adapter() {
        // hang_adapter.py sleeps forever; a short budget must surface a timeout
        // error (and kill_on_drop reaps the child) rather than hang the test.
        let err = run_adapter_to(
            &fixtures_dir(),
            "hang_adapter",
            &json!({"op": "x"}),
            std::time::Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "got: {err}");
    }
}

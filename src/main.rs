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

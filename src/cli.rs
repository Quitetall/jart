//! CLI entrypoint shared by the `jart` and `research` binaries (same tool, two
//! names). Both `src/main.rs` and `src/bin/research.rs` just call `run()`.

use crate::core::ai::AiClient;
use crate::core::cache::Cache;
use crate::core::config::Config;
use crate::core::feed;
use crate::core::ratelimit::Pacer;
use crate::server::{router, AppState};
use crate::tui::{self, TuiConfig};
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "jart", about = "jart — Just Another Research Tool (local research aggregator)")]
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
    /// Config file path (default: $XDG_CONFIG_HOME/jart/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Skip the TUI: serve the web GUI and open it in a browser.
    #[arg(long)]
    web: bool,
}

// NOTE: CARGO_MANIFEST_DIR is only the *default* — it bakes in the build-time
// source path, which is wrong after `cargo install` to another location. The
// `--scrapers-dir` / `--dist-dir` flags override it. A later "Install" pass
// replaces these defaults with an install-aware resolver.
fn default_scrapers_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scrapers")
}
fn default_dist_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

#[tokio::main]
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(cli.config.clone());
    let ai = Arc::new(AiClient::new(cfg.lamu_url.clone(), cfg.model.clone()));
    let scrapers = cli.scrapers_dir.clone().unwrap_or_else(default_scrapers_dir);
    let dist = cli.dist_dir.clone().unwrap_or_else(default_dist_dir);
    // One shared cache + pacer for every surface (server, --check, TUI): the
    // pacer's per-source state and the disk cache must be process-wide.
    let cache = Arc::new(Cache::new());
    let pacer = Arc::new(Pacer::new());

    if cli.check {
        let feed = feed::load(&scrapers, &cfg.topics()[..1], 3, &cache, &pacer).await;
        println!("feed: {} papers, {} errors", feed.papers.len(), feed.errors.len());
        for e in &feed.errors {
            println!("  ERR {}: {}", e.source, e.message);
        }
        if let Some(p) = feed.papers.first() {
            match ai
                .summarize("One sentence on this paper:", &[format!("{}\n{}", p.title, p.grounding)])
                .await
            {
                Ok(txt) => println!("ai ok: {}", txt.lines().next().unwrap_or("")),
                Err(e) => println!("ai ERR (LAMU up?): {e}"),
            }
        }
        return Ok(());
    }

    let topics = cfg.topics();
    let addr = format!("127.0.0.1:{}", cfg.web_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let web_url = format!("http://{addr}");

    // The web server runs in the background for BOTH modes: the TUI's `w` key
    // opens it on demand; `--web` opens it directly.
    let state = AppState {
        scrapers_dir: scrapers.clone(),
        topics: topics.clone(),
        ai: ai.clone(),
        dist_dir: dist,
        cache: cache.clone(),
        pacer: pacer.clone(),
    };
    let server = router(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, server).await;
    });

    if cli.web {
        let _ = std::process::Command::new("xdg-open").arg(&web_url).spawn();
        println!("jart web GUI on {web_url}  (Ctrl-C to stop)");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // Default: the TUI is the primary surface.
    tui::run(TuiConfig { scrapers_dir: scrapers, topics, ai, web_url, cache, pacer }).await
}

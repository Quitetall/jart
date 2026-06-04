//! Ratatui TUI — the primary surface. Shows the live feed, opens the web GUI on
//! demand, and runs AI summaries, all over the same `core` as the web server.
//!
//! Keys: ↑/↓ (or k/j) move · Enter open paper · w open web GUI · s summarize ·
//! r reload · q / Ctrl-C quit · Esc clear summary.

use crate::core::ai::AiClient;
use crate::core::config::Topic;
use crate::core::feed;
use crate::core::model::{Feed, Paper};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

const SUMMARY_PROMPT: &str =
    "Synthesize the newest papers below into 2-3 short paragraphs: dominant themes, \
     anything new or surprising, and active directions. Ground claims only in the text. No preamble.";

/// Everything the TUI needs; mirrors the web server's `AppState`.
pub struct TuiConfig {
    pub scrapers_dir: PathBuf,
    pub topics: Vec<Topic>,
    pub ai: Arc<AiClient>,
    pub web_url: String,
}

/// Async results delivered back to the UI loop.
enum Msg {
    Feed(Feed),
    Summary(std::result::Result<String, String>),
}

/// One display line for a paper (also unit-tested).
fn paper_line(p: &Paper) -> String {
    let date = if p.date_label.is_empty() { "—" } else { &p.date_label };
    format!("{:9} {:>10}  {}", p.source, date, p.title)
}

/// Build the AI grounding items from the feed (also unit-tested).
fn summary_items(papers: &[Paper], n: usize) -> Vec<String> {
    papers
        .iter()
        .take(n)
        .map(|p| {
            let body = if p.grounding.is_empty() { &p.summary } else { &p.grounding };
            format!("Title: {}\nAbstract: {}", p.title, body.chars().take(700).collect::<String>())
        })
        .collect()
}

/// Best-effort open; returns whether the launcher spawned so the caller can
/// give honest status (xdg-open may be absent).
fn open_in_browser(url: &str) -> bool {
    std::process::Command::new("xdg-open").arg(url).spawn().is_ok()
}

/// Restores the terminal on ANY exit from `run` — normal return, `?`, or panic.
struct TermGuard;
impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

struct App {
    scrapers_dir: PathBuf,
    topics: Vec<Topic>,
    ai: Arc<AiClient>,
    web_url: String,
    feed: Feed,
    list: ListState,
    status: String,
    summary: Option<String>,
    loading: bool,
    summarizing: bool,
}

impl App {
    fn new(cfg: TuiConfig) -> Self {
        App {
            scrapers_dir: cfg.scrapers_dir,
            topics: cfg.topics,
            ai: cfg.ai,
            web_url: cfg.web_url,
            feed: Feed::default(),
            list: ListState::default(),
            status: "starting…".into(),
            summary: None,
            loading: false,
            summarizing: false,
        }
    }

    fn reload(&mut self, tx: &mpsc::Sender<Msg>) {
        if self.loading {
            return; // a load is already in flight; ignore reload spam
        }
        self.loading = true;
        self.status = "loading papers…".into();
        let (dir, topics, tx) = (self.scrapers_dir.clone(), self.topics.clone(), tx.clone());
        tokio::spawn(async move {
            let feed = feed::load(&dir, &topics, 8).await;
            let _ = tx.send(Msg::Feed(feed)).await;
        });
    }

    fn summarize(&mut self, tx: &mpsc::Sender<Msg>) {
        if self.feed.papers.is_empty() || self.summarizing {
            return;
        }
        self.summarizing = true;
        self.summary = Some("summarizing…".into());
        let items = summary_items(&self.feed.papers, 14);
        let (ai, tx) = (self.ai.clone(), tx.clone());
        tokio::spawn(async move {
            let res = ai.summarize(SUMMARY_PROMPT, &items).await.map_err(|e| e.to_string());
            let _ = tx.send(Msg::Summary(res)).await;
        });
    }

    fn select(&mut self, delta: isize) {
        let n = self.feed.papers.len();
        if n == 0 {
            return;
        }
        let cur = self.list.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n as isize) as usize;
        self.list.select(Some(next));
    }

    /// Returns true when the app should quit.
    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers, tx: &mpsc::Sender<Msg>) -> bool {
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.summary = None, // clear-only; q / Ctrl-C quit
            KeyCode::Char('r') => self.reload(tx),
            KeyCode::Char('w') => {
                self.status = if open_in_browser(&self.web_url) {
                    format!("opened {}", self.web_url)
                } else {
                    "couldn't launch browser (is xdg-open installed?)".into()
                };
            }
            KeyCode::Char('s') => self.summarize(tx),
            KeyCode::Down | KeyCode::Char('j') => self.select(1),
            KeyCode::Up | KeyCode::Char('k') => self.select(-1),
            KeyCode::Enter => {
                if let Some(i) = self.list.selected() {
                    if let Some(p) = self.feed.papers.get(i) {
                        if !p.link.is_empty() {
                            self.status = if open_in_browser(&p.link) {
                                "opened paper".into()
                            } else {
                                "couldn't launch browser".into()
                            };
                        }
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Feed(feed) => {
                self.feed = feed;
                self.loading = false;
                if !self.feed.papers.is_empty() && self.list.selected().is_none() {
                    self.list.select(Some(0));
                }
                let errs = self.feed.errors.len();
                self.status = if errs > 0 {
                    format!("{} papers · {} source error(s)", self.feed.papers.len(), errs)
                } else {
                    format!("{} papers", self.feed.papers.len())
                };
            }
            Msg::Summary(res) => {
                self.summarizing = false;
                self.summary = Some(match res {
                    Ok(t) => t,
                    Err(e) => format!("summary failed: {e}"),
                });
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    // Title / status bar.
    let head = format!(
        "research · {}{}",
        if app.loading { "loading… · " } else { "" },
        app.status
    );
    f.render_widget(
        Paragraph::new(head).style(Style::default().fg(Color::Black).bg(Color::Cyan)),
        chunks[0],
    );

    // Body: list, plus a summary pane when present.
    let body = if app.summary.is_some() {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1])
    } else {
        Layout::default().constraints([Constraint::Percentage(100)]).split(chunks[1])
    };

    let items: Vec<ListItem> = app.feed.papers.iter().map(|p| ListItem::new(paper_line(p))).collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Newest papers "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, body[0], &mut app.list);

    if let Some(text) = &app.summary {
        let para = Paragraph::new(text.as_str())
            .block(Block::default().borders(Borders::ALL).title(" AI summary "))
            .wrap(Wrap { trim: true });
        f.render_widget(para, body[1]);
    }

    // Footer keybinds.
    let foot = "[↑/↓] move  [enter] open  [w] web GUI  [s] summarize  [r] reload  [esc] clear  [q] quit";
    f.render_widget(
        Paragraph::new(foot).style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

/// Run the TUI to completion. Restores the terminal even on error.
pub async fn run(cfg: TuiConfig) -> Result<()> {
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    // From here on, the terminal is restored on every exit path (incl. panic).
    let _guard = TermGuard;

    let mut term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let (tx, mut rx) = mpsc::channel::<Msg>(16);
    let mut app = App::new(cfg);
    app.reload(&tx);
    let mut events = EventStream::new();

    run_loop(&mut term, &mut app, &mut events, &tx, &mut rx).await
}

async fn run_loop(
    term: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    events: &mut EventStream,
    tx: &mpsc::Sender<Msg>,
    rx: &mut mpsc::Receiver<Msg>,
) -> Result<()> {
    loop {
        term.draw(|f| ui(f, app))?;
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => {
                    if app.on_key(k.code, k.modifiers, tx) {
                        return Ok(());
                    }
                }
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
                _ => {}
            },
            Some(msg) = rx.recv() => app.on_msg(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper(title: &str, src: &str, date: &str) -> Paper {
        Paper {
            kind: "paper".into(), source: src.into(), topic: "T".into(),
            title: title.into(), link: "https://x/1".into(), date_label: date.into(),
            ts: 1, summary: "s".into(), grounding: "g".into(),
        }
    }

    #[test]
    fn paper_line_includes_source_date_title() {
        let l = paper_line(&paper("Deep EEG", "HF", "2026-05-01"));
        assert!(l.contains("HF"));
        assert!(l.contains("2026-05-01"));
        assert!(l.contains("Deep EEG"));
    }

    #[test]
    fn paper_line_uses_dash_for_missing_date() {
        let l = paper_line(&paper("X", "PubMed", ""));
        assert!(l.contains("—"));
    }

    #[test]
    fn summary_items_caps_count_and_prefers_grounding() {
        let papers: Vec<Paper> = (0..20).map(|i| paper(&format!("P{i}"), "HF", "2026-01-01")).collect();
        let items = summary_items(&papers, 14);
        assert_eq!(items.len(), 14);
        assert!(items[0].contains("Abstract: g")); // grounding preferred over summary
        assert!(items[0].contains("Title: P0"));
    }
}

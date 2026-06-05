//! Per-source minimum-interval pacer (spec §5.2, plan §5d).
//!
//! The Python adapters are stateless single-shot processes (spec §4.0), so they
//! can't pace themselves across calls. The Rust host is the persistent process,
//! so the rate limiter lives here. `acquire(source)` blocks until at least
//! `interval[source]` has elapsed since the last `acquire` for that source,
//! then stamps "now" — serializing back-to-back fetches of the same source.

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Default per-source minimum intervals between requests.
fn default_intervals() -> HashMap<String, Duration> {
    // PubMed E-utilities: 3 req/s without a key, 10 req/s with NCBI_API_KEY.
    // Each pubmed task makes TWO calls (esearch + efetch), and the adapter sleeps
    // ~350ms between them when unkeyed; pacing spawns ~700ms apart then keeps the
    // combined rate under 3/s across concurrent topics. With a key, 110ms.
    let pubmed = if std::env::var_os("NCBI_API_KEY").is_some() {
        Duration::from_millis(110)
    } else {
        Duration::from_millis(700)
    };
    HashMap::from([
        ("pubmed".to_string(), pubmed),
        ("semantic".to_string(), Duration::from_millis(1100)),
        ("biorxiv".to_string(), Duration::from_millis(250)),
        ("huggingface".to_string(), Duration::from_millis(150)),
    ])
}

/// Default interval for a source with no explicit entry.
const DEFAULT_INTERVAL: Duration = Duration::from_millis(200);

/// Persistent per-source pacer. Cheap to `Arc`-share across the server + TUI.
pub struct Pacer {
    last: Mutex<HashMap<String, Instant>>,
    intervals: HashMap<String, Duration>,
}

impl Default for Pacer {
    fn default() -> Self {
        Pacer::new()
    }
}

impl Pacer {
    /// Pacer with the shipped per-source intervals.
    pub fn new() -> Self {
        Pacer {
            last: Mutex::new(HashMap::new()),
            intervals: default_intervals(),
        }
    }

    /// Pacer with caller-supplied intervals (tests inject a short interval).
    pub fn with_intervals(intervals: HashMap<String, Duration>) -> Self {
        Pacer {
            last: Mutex::new(HashMap::new()),
            intervals,
        }
    }

    fn interval_for(&self, source: &str) -> Duration {
        self.intervals.get(source).copied().unwrap_or(DEFAULT_INTERVAL)
    }

    /// Block until this source is allowed to fire, then stamp the firing time.
    ///
    /// The deadline is *reserved* under the lock, but the sleep happens **after**
    /// the lock is dropped. This matters two ways:
    ///   - Different sources never block each other: while the HF task sleeps,
    ///     the lock is free, so PubMed/Semantic/bioRxiv proceed in parallel.
    ///     (Holding the lock across the sleep would serialize the whole fan-out
    ///     through one mutex — defeating the concurrent `join_all`.)
    ///   - Concurrent acquires of the *same* source still space out: each stamps
    ///     the next slot (`now`, `now+interval`, `now+2·interval`, …) while
    ///     holding the lock briefly, then sleeps to its own deadline.
    pub async fn acquire(&self, source: &str) {
        let interval = self.interval_for(source);
        let ready = {
            let mut last = self.last.lock().await;
            let now = Instant::now();
            // First acquire of a source: fire now. Otherwise the next free slot
            // is `max(prev_reserved + interval, now)`.
            let ready = last.get(source).map_or(now, |&prev| (prev + interval).max(now));
            last.insert(source.to_string(), ready); // reserve before releasing
            ready
        }; // lock dropped here — other sources are unblocked during the sleep
        tokio::time::sleep_until(ready).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn back_to_back_acquires_span_the_interval() {
        let interval = Duration::from_millis(120);
        let pacer = Pacer::with_intervals(HashMap::from([("x".to_string(), interval)]));
        let start = Instant::now();
        pacer.acquire("x").await; // first: no prior stamp, returns immediately
        pacer.acquire("x").await; // second: must wait ~interval
        let elapsed = start.elapsed();
        assert!(
            elapsed >= interval,
            "two acquires spanned {elapsed:?}, expected >= {interval:?}"
        );
    }

    #[tokio::test]
    async fn distinct_sources_do_not_block_each_other_under_contention() {
        // Exercise the *contended* path the old test missed: prime "a" so a
        // second acquire("a") must sleep its full interval, then run that sleepy
        // acquire concurrently with acquire("b"). If the pacer held one global
        // lock across the sleep, "b" would wait on "a"; it must not.
        let interval = Duration::from_millis(400);
        let pacer = Arc::new(Pacer::with_intervals(HashMap::from([
            ("a".to_string(), interval),
            ("b".to_string(), interval),
        ])));
        pacer.acquire("a").await; // prime "a": next acquire("a") sleeps ~interval

        let pa = pacer.clone();
        let a_task = tokio::spawn(async move { pa.acquire("a").await }); // will sleep
        let start = Instant::now();
        pacer.acquire("b").await; // distinct source: must NOT be gated on "a"
        let b_elapsed = start.elapsed();
        assert!(
            b_elapsed < interval / 2,
            "acquire(b) took {b_elapsed:?}; a distinct source must not block on a sleeping one"
        );
        a_task.await.unwrap();
    }

    #[tokio::test]
    async fn unknown_source_uses_default_interval() {
        let pacer = Pacer::with_intervals(HashMap::new());
        // No entry -> DEFAULT_INTERVAL; just assert it doesn't panic and is paced.
        assert_eq!(pacer.interval_for("whatever"), DEFAULT_INTERVAL);
    }
}

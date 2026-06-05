//! On-disk result cache with per-source TTL (spec §5, plan §5c).
//!
//! Layout: `<root>/<source>/<hashed-key>.json`. A `get` is a hit only when the
//! file exists AND its mtime is within `ttl`. Every IO error is non-fatal — a
//! cache miss/failure must never take down a feed load, so `get` returns `None`
//! and `put` silently ignores. No new crates: the key is hashed to a filename
//! with the std `DefaultHasher`.

use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Disk-backed cache rooted at a directory (one subdir per source).
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new()
    }
}

impl Cache {
    /// Cache under `$XDG_CACHE_HOME/jart` (or `~/.cache/jart`). If neither env
    /// var is set, falls back to the system temp dir so the ctor is always
    /// infallible.
    pub fn new() -> Self {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join("jart");
        Cache::with_root(root)
    }

    /// Cache rooted at an explicit directory (tests point this at a temp dir).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Cache { root: root.into() }
    }

    /// Path of the cache file for `(source, key)`. The key is hashed (not
    /// embedded) so arbitrary query strings can't escape the source subdir or
    /// blow past filesystem name limits.
    fn path(&self, source: &str, key: &str) -> PathBuf {
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        let file = format!("{:016x}.json", h.finish());
        // `source` is a fixed adapter name from our own call sites, but sanitize
        // defensively so a stray separator can't redirect the write.
        let safe_source: String = source
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect();
        self.root.join(safe_source).join(file)
    }

    /// Cached value for `(source, key)` if present and fresher than `ttl`,
    /// else `None`. Any IO/parse failure (missing file, bad mtime, corrupt
    /// JSON) is treated as a miss.
    pub fn get(&self, source: &str, key: &str, ttl: Duration) -> Option<Value> {
        let path = self.path(source, key);
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).unwrap_or(Duration::MAX);
        if age > ttl {
            return None; // stale
        }
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Store `val` for `(source, key)`. Best-effort: any IO error is swallowed
    /// so a failed write degrades to "no cache", never a crash.
    pub fn put(&self, source: &str, key: &str, val: &Value) {
        let path = self.path(source, key);
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        if let Ok(bytes) = serde_json::to_vec(val) {
            let _ = std::fs::write(&path, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A unique temp root per test so parallel runs don't collide.
    fn temp_root(tag: &str) -> PathBuf {
        let mut h = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut h);
        tag.hash(&mut h);
        std::env::temp_dir().join(format!("research_cache_test_{:016x}", h.finish()))
    }

    #[test]
    fn put_then_get_is_a_hit() {
        let root = temp_root("hit");
        let cache = Cache::with_root(&root);
        let val = json!({"records": [{"title": "X"}]});
        cache.put("PubMed", "eeg seizure", &val);
        let got = cache.get("PubMed", "eeg seizure", Duration::from_secs(3600));
        assert_eq!(got, Some(val));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn zero_ttl_is_a_miss() {
        let root = temp_root("miss");
        let cache = Cache::with_root(&root);
        cache.put("HF", "k", &json!({"a": 1}));
        // A zero TTL means anything already on disk is, by definition, too old.
        assert_eq!(cache.get("HF", "k", Duration::from_secs(0)), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_key_is_a_miss() {
        let cache = Cache::with_root(temp_root("absent"));
        assert_eq!(cache.get("HF", "never-written", Duration::from_secs(3600)), None);
    }
}

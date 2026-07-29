use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::cache::AppCache;
use crate::lyrics::LyricLine;

pub struct LyricsCache {
    cache: AppCache,
}

impl LyricsCache {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            cache: AppCache::new()?,
        })
    }

    pub fn get(&self, title: &str, artist: &str, duration_ms: u64) -> Option<Vec<LyricLine>> {
        let path = self.cache.path_for(&Self::key(title, artist, duration_ms));
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn set(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
        lines: &[LyricLine],
    ) -> anyhow::Result<()> {
        let path = self.cache.path_for(&Self::key(title, artist, duration_ms));
        let raw = serde_json::to_string(lines)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    /// Hashed instead of a literal `{artist} - {title}` filename: Windows forbids
    /// `/ : ? * " < > |`, which real track/artist names can contain.
    fn key(title: &str, artist: &str, duration_ms: u64) -> String {
        let mut hasher = DefaultHasher::new();
        title.hash(&mut hasher);
        artist.hash(&mut hasher);
        duration_ms.hash(&mut hasher);
        format!("{:016x}.json", hasher.finish())
    }
}

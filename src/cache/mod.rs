use std::path::PathBuf;

use anyhow::Context;

pub struct AppCache {
    dir: PathBuf,
}

impl AppCache {
    pub fn new() -> anyhow::Result<Self> {
        let dirs = directories::ProjectDirs::from("dev", "lyra", "lyra")
            .context("could not determine a cache directory for this platform")?;
        let dir = dirs.cache_dir().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(key)
    }
}

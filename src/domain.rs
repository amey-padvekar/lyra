#[derive(Clone, Debug)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub position_ms: u64,
    pub received_at: std::time::Instant,
}

impl NowPlaying {
    pub fn track_id(&self) -> (&str, &str, u64) {
        (&self.title, &self.artist, self.duration_ms)
    }
}

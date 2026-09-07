#[derive(Clone, Debug)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub position_ms: u64,
    pub received_at: std::time::Instant,
    /// What the source says it will actually honour. Not every player supports
    /// every transport command — browsers only expose next/previous when the
    /// page registers media-session handlers — so the UI greys out what would
    /// otherwise be buttons that silently do nothing.
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    /// Whether the source honours a jump to an arbitrary position. Kept apart
    /// from the transport flags above because they don't imply each other: a
    /// live stream happily accepts next/previous but has nowhere to scrub to.
    pub can_seek: bool,
}

impl NowPlaying {
    pub fn track_id(&self) -> (&str, &str, u64) {
        (&self.title, &self.artist, self.duration_ms)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub position_ms: u64,
}

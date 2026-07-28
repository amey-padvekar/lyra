pub mod cache;
pub mod fetcher;
pub mod parser;

#[derive(Clone, Debug)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

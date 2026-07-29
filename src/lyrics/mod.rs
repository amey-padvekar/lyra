pub mod cache;
pub mod fetcher;
pub mod parser;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

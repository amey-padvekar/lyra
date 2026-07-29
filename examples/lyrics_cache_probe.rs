// Throwaway probe verifying LyricsCache round-trips through disk correctly.
// Run with: cargo run --example lyrics_cache_probe

use lyra::lyrics::cache::LyricsCache;
use lyra::lyrics::LyricLine;

fn main() {
    let cache = LyricsCache::new().expect("cache dir");
    let title = "Probe Track";
    let artist = "Probe Artist";
    let duration_ms = 123_456u64;

    assert!(cache.get(title, artist, duration_ms).is_none());

    let lines = vec![
        LyricLine {
            time_ms: 0,
            text: "line a".to_string(),
        },
        LyricLine {
            time_ms: 1_000,
            text: "line b".to_string(),
        },
    ];
    cache.set(title, artist, duration_ms, &lines).expect("set");

    let read_back = cache.get(title, artist, duration_ms).expect("cache hit");
    assert_eq!(read_back, lines);
    println!("cache round-trip ok: {read_back:?}");
}

// Throwaway probe for Step 3's gate (see IMPLEMENTATION_PLAN.md).
// Run with: cargo run --example lrclib_probe
// Fetches a few well-known tracks from LRCLIB and prints how many lines came back.

use lyra::lyrics::fetcher::LyricsFetcher;

fn main() {
    let candidates = [
        ("Yesterday", "The Beatles", 125_000u64),
        ("Numb", "Linkin Park", 187_000u64),
        ("Take On Me", "a-ha", 225_000u64),
        // Deliberately wrong duration so LRCLIB can't match — forces the
        // lyrics.ovh plain-text fallback path.
        ("Bohemian Rhapsody", "Queen", 1_000u64),
    ];

    for (title, artist, duration_ms) in candidates {
        match LyricsFetcher::fetch(title, artist, duration_ms) {
            Ok(lines) => println!(
                "{title} - {artist}: {} lines (first: {:?})",
                lines.len(),
                lines.first()
            ),
            Err(e) => println!("{title} - {artist}: error: {e}"),
        }
    }
}

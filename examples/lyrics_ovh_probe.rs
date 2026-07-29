// Throwaway probe verifying lyrics.ovh URL handling, including whether a
// literal '/' in an artist name (e.g. "AC/DC") needs manual percent-encoding
// before being embedded in the path, or whether ureq/url handles it safely.
// Run with: cargo run --example lyrics_ovh_probe

use serde::Deserialize;

#[derive(Deserialize)]
struct LyricsOvhTrack {
    lyrics: String,
}

fn percent_encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn try_fetch(label: &str, url: &str) {
    match ureq::get(url).call() {
        Ok(response) => match response.into_json::<LyricsOvhTrack>() {
            Ok(track) => {
                let preview: String = track.lyrics.chars().take(60).collect();
                println!("{label}: OK, {} chars, starts: {preview:?}", track.lyrics.len());
            }
            Err(e) => println!("{label}: json error: {e}"),
        },
        Err(e) => println!("{label}: request error: {e}"),
    }
}

fn main() {
    let artist = "AC/DC";
    let title = "Highway to Hell";

    let raw_url = format!("https://api.lyrics.ovh/v1/{artist}/{title}");
    println!("raw url:     {raw_url}");
    try_fetch("raw (unencoded '/')", &raw_url);

    let encoded_url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        percent_encode_path_segment(artist),
        percent_encode_path_segment(title)
    );
    println!("encoded url: {encoded_url}");
    try_fetch("percent-encoded", &encoded_url);
}

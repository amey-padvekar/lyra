use serde::Deserialize;

use crate::lyrics::{parser, LyricLine};

pub struct LyricsFetcher;

impl LyricsFetcher {
    pub fn fetch(title: &str, artist: &str, duration_ms: u64) -> anyhow::Result<Vec<LyricLine>> {
        match Self::fetch_lrclib(title, artist, duration_ms) {
            Ok(lines) if !lines.is_empty() => return Ok(lines),
            Ok(_) => {}
            Err(e) => crate::log!("lyrics: lrclib lookup failed for {title} - {artist}: {e}"),
        }

        Self::fetch_lyrics_ovh(title, artist)
    }

    fn fetch_lrclib(title: &str, artist: &str, duration_ms: u64) -> anyhow::Result<Vec<LyricLine>> {
        let duration_s = duration_ms / 1_000;
        let track: LrclibTrack = ureq::get("https://lrclib.net/api/get")
            .set("User-Agent", "lyra/0.1 (github.com/lyra-app/lyra)")
            .query("track_name", title)
            .query("artist_name", artist)
            .query("duration", &duration_s.to_string())
            .call()?
            .into_json()?;

        if let Some(synced) = track.synced_lyrics.filter(|s| !s.is_empty()) {
            return Ok(parser::parse_lrc(&synced));
        }
        if let Some(plain) = track.plain_lyrics.filter(|s| !s.is_empty()) {
            return Ok(vec![LyricLine {
                time_ms: 0,
                text: plain,
            }]);
        }
        Ok(Vec::new())
    }

    /// Unsynced fallback for tracks LRCLIB doesn't have. lyrics.ovh only
    /// returns plain text (no timestamps), so the whole lyric is surfaced as a
    /// single `time_ms: 0` line — the same shape `lyra.md` specifies for
    /// LRCLIB's own plain-lyrics case.
    fn fetch_lyrics_ovh(title: &str, artist: &str) -> anyhow::Result<Vec<LyricLine>> {
        let url = format!(
            "https://api.lyrics.ovh/v1/{}/{}",
            percent_encode_path_segment(artist),
            percent_encode_path_segment(title)
        );
        let response = match ureq::get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let track: LyricsOvhTrack = response.into_json()?;
        let text = track.lyrics.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![LyricLine {
            time_ms: 0,
            text: text.to_string(),
        }])
    }
}

/// lyrics.ovh takes the artist/title as URL path segments, not query
/// parameters, so `ureq`'s own query-string escaping doesn't apply — this
/// percent-encodes each UTF-8 byte outside the URL-safe unreserved set.
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

#[derive(Deserialize)]
struct LrclibTrack {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

#[derive(Deserialize)]
struct LyricsOvhTrack {
    lyrics: String,
}

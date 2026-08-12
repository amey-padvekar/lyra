//! Reads macOS's system-level Now Playing session via the `media-control`
//! CLI (<https://github.com/ungive/media-control>), when installed. That
//! session is populated by any app or browser tab via `MPNowPlayingInfoCenter`
//! / the Media Session API, so — unlike AppleScript window-title scraping in
//! `macos::AppleScriptReader` — it works regardless of which app or tab
//! currently has focus. `media-control` itself works around Apple's
//! post-macOS-15.4 lockdown of the private `MediaRemote.framework` by
//! shelling out through a signed system binary; see `HybridReader` in
//! `macos.rs` for how this combines with the AppleScript fallback when the
//! tool isn't installed.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use serde_json::Value;

use crate::domain::NowPlaying;
use crate::media::MediaCommand;

/// Bare name first — resolved via `PATH`, which covers `cargo run`/terminal
/// launches — then the two standard Homebrew install locations, since
/// GUI-launched apps often have a `PATH` that doesn't include Homebrew's bin
/// dir at all.
const CANDIDATES: [&str; 3] = [
    "media-control",
    "/opt/homebrew/bin/media-control",
    "/usr/local/bin/media-control",
];

pub struct MediaRemoteAdapter {
    latest: Arc<Mutex<Option<NowPlaying>>>,
}

impl MediaRemoteAdapter {
    /// `None` if `media-control` isn't installed anywhere we looked. The
    /// caller then falls back to AppleScript for this whole run rather than
    /// repeatedly retrying a tool that isn't there.
    pub fn spawn() -> Option<Self> {
        let mut child = CANDIDATES.iter().find_map(|path| {
            Command::new(path)
                .args(["stream", "--no-diff", "--micros", "--no-artwork"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()
        })?;

        let stdout = child.stdout.take()?;
        let latest = Arc::new(Mutex::new(None));
        let latest_writer = latest.clone();

        thread::spawn(move || {
            // Held only to keep the child alive for as long as this thread
            // reads its stdout; never read again. Dropping it early would
            // let the OS reap the process out from under the open pipe.
            let _child = child;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let payload = value.get("payload").unwrap_or(&value);
                let parsed = parse_payload(payload, Instant::now());
                if let Ok(mut guard) = latest_writer.lock() {
                    *guard = parsed;
                }
            }
        });

        Some(Self { latest })
    }

    pub fn latest(&self) -> Option<NowPlaying> {
        self.latest.lock().ok()?.clone()
    }

    pub fn send(&self, command: MediaCommand) -> anyhow::Result<()> {
        let subcommand = match command {
            MediaCommand::TogglePlayPause => "toggle-play-pause",
            MediaCommand::Next => "next-track",
            MediaCommand::Previous => "previous-track",
        };
        let output = CANDIDATES
            .iter()
            .find_map(|path| Command::new(path).arg(subcommand).output().ok())
            .ok_or_else(|| anyhow::anyhow!("media-control not found"))?;
        if !output.status.success() {
            anyhow::bail!(
                "media-control {subcommand} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

/// Pure and unit-testable: maps one `media-control --micros` payload object
/// into a `NowPlaying`. `bundleIdentifier`, `playing`, and `title` are the
/// only keys documented as always present when the payload is non-empty;
/// everything else is optional and defaulted — `duration`/`elapsedTime`
/// default to `0` when absent, which `SyncEngine` already treats as "unknown
/// position source" rather than a literal zero-length track.
fn parse_payload(payload: &Value, received_at: Instant) -> Option<NowPlaying> {
    let title = payload.get("title")?.as_str()?.to_string();
    if title.trim().is_empty() {
        return None;
    }
    let is_playing = payload.get("playing")?.as_bool()?;

    let artist = payload
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let duration_ms = micros_field(payload, "durationMicros");
    let position_ms = micros_field(payload, "elapsedTimeMicros");

    Some(NowPlaying {
        title,
        artist,
        duration_ms,
        is_playing,
        position_ms,
        received_at,
        can_play_pause: true,
        can_next: true,
        can_previous: true,
    })
}

fn micros_field(payload: &Value, key: &str) -> u64 {
    payload
        .get(key)
        .and_then(Value::as_f64)
        .map_or(0, |micros| (micros / 1_000.0).max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_full_payload() {
        let payload = json!({
            "bundleIdentifier": "com.spotify.client",
            "playing": true,
            "title": "Song",
            "artist": "Artist",
            "durationMicros": 200_000_000u64,
            "elapsedTimeMicros": 45_000_000u64,
        });

        let np = parse_payload(&payload, Instant::now()).expect("should parse");
        assert_eq!(np.title, "Song");
        assert_eq!(np.artist, "Artist");
        assert_eq!(np.duration_ms, 200_000);
        assert_eq!(np.position_ms, 45_000);
        assert!(np.is_playing);
    }

    #[test]
    fn missing_title_is_rejected() {
        let payload = json!({ "bundleIdentifier": "com.spotify.client", "playing": true });
        assert!(parse_payload(&payload, Instant::now()).is_none());
    }

    #[test]
    fn empty_payload_is_rejected() {
        assert!(parse_payload(&json!({}), Instant::now()).is_none());
    }

    #[test]
    fn missing_artist_and_duration_default_sanely() {
        let payload = json!({
            "bundleIdentifier": "com.example.app",
            "playing": false,
            "title": "Untitled",
        });

        let np = parse_payload(&payload, Instant::now()).expect("should parse");
        assert_eq!(np.artist, "");
        assert_eq!(np.duration_ms, 0);
        assert_eq!(np.position_ms, 0);
        assert!(!np.is_playing);
    }
}

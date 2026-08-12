use std::process::Command;
use std::time::Instant;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::domain::NowPlaying;
use crate::media::mediaremote;
use crate::media::{MediaCommand, MediaReader};

/// AppleScript/`osascript`-driven fallback reader. Despite the historical
/// name this predates, it is *not* MediaRemote-based — see
/// `mediaremote::MediaRemoteAdapter` for the real thing and `HybridReader`
/// for how the two combine.
pub struct AppleScriptReader;

#[derive(Deserialize)]
struct MacNowPlaying {
    title: String,
    artist: String,
    duration_ms: u64,
    position_ms: u64,
    is_playing: bool,
    can_play_pause: bool,
    can_next: bool,
    can_previous: bool,
}

impl MediaReader for AppleScriptReader {
    fn poll(&mut self) -> anyhow::Result<Option<NowPlaying>> {
        let output = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", NOW_PLAYING_SCRIPT])
            .output()
            .context("failed to run osascript for macOS media poll")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("osascript media poll failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let payload = stdout.trim();
        if payload.is_empty() || payload == "null" {
            return Ok(None);
        }

        let current: MacNowPlaying =
            serde_json::from_str(payload).context("failed to parse osascript now-playing JSON")?;

        Ok(Some(NowPlaying {
            title: current.title,
            artist: current.artist,
            duration_ms: current.duration_ms,
            is_playing: current.is_playing,
            position_ms: current.position_ms,
            received_at: Instant::now(),
            can_play_pause: current.can_play_pause,
            can_next: current.can_next,
            can_previous: current.can_previous,
        }))
    }

    fn execute(&mut self, command: MediaCommand) -> anyhow::Result<()> {
        let script = match command {
            MediaCommand::TogglePlayPause => EXECUTE_TOGGLE_SCRIPT,
            MediaCommand::Next => EXECUTE_NEXT_SCRIPT,
            MediaCommand::Previous => EXECUTE_PREVIOUS_SCRIPT,
        };

        let output = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", script])
            .output()
            .context("failed to run osascript for macOS media command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "osascript media command {:?} failed: {}",
                command,
                stderr.trim()
            );
        }

        let accepted = String::from_utf8_lossy(&output.stdout).trim() == "true";
        if !accepted {
            crate::log!("media: source declined {command:?}");
        }
        Ok(())
    }
}

/// Prefers the system-level Now Playing session (via `media-control`, when
/// installed) over AppleScript automation: it reflects whatever the OS
/// considers "now playing" regardless of which app or browser tab is
/// currently focused, which AppleScript window/tab scraping fundamentally
/// cannot see. Falls back to `AppleScriptReader` whenever the adapter has
/// nothing to report — either because it isn't installed, or because nothing
/// is registered with the system media session at all.
pub struct HybridReader {
    adapter: Option<mediaremote::MediaRemoteAdapter>,
    fallback: AppleScriptReader,
}

impl HybridReader {
    pub fn new() -> Self {
        Self {
            adapter: mediaremote::MediaRemoteAdapter::spawn(),
            fallback: AppleScriptReader,
        }
    }
}

impl Default for HybridReader {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaReader for HybridReader {
    fn poll(&mut self) -> anyhow::Result<Option<NowPlaying>> {
        if let Some(adapter) = &self.adapter
            && let Some(np) = adapter.latest()
        {
            return Ok(Some(np));
        }
        self.fallback.poll()
    }

    fn execute(&mut self, command: MediaCommand) -> anyhow::Result<()> {
        if let Some(adapter) = &self.adapter {
            match adapter.send(command) {
                Ok(()) => return Ok(()),
                Err(e) => crate::log!("media-control command {command:?} failed, falling back to AppleScript: {e}"),
            }
        }
        self.fallback.execute(command)
    }
}

const NOW_PLAYING_SCRIPT: &str = r#"
function callOr(f, fallback) {
  try {
    return f();
  } catch (e) {
    return fallback;
  }
}

function readMusicLike(name) {
  try {
    const app = Application(name);
    if (!app.running()) {
      return null;
    }
    const state = String(callOr(() => app.playerState(), ""));
    const track = app.currentTrack();
    if (!track) {
      return null;
    }
    const title = String(callOr(() => track.name(), ""));
    if (title.trim() === "") {
      return null;
    }
    const artist = String(callOr(() => track.artist(), ""));
    return {
      title: title,
      artist: artist,
      duration_ms: Math.max(0, Math.round(Number(callOr(() => track.duration(), 0) || 0) * 1000)),
      position_ms: Math.max(0, Math.round(Number(callOr(() => app.playerPosition(), 0) || 0) * 1000)),
      is_playing: state === "playing",
      can_play_pause: true,
      can_next: true,
      can_previous: true
    };
  } catch (e) {
    return null;
  }
}

function readVlc() {
  try {
    const app = Application("VLC");
    if (!app.running()) {
      return null;
    }

    const title = String(
      callOr(() => app.nameOfCurrentItem(), "")
      || callOr(() => app.currentItem().name(), "")
      || ""
    );
    if (title.trim() === "") {
      return null;
    }

    const artist = String(
      callOr(() => app.artistOfCurrentItem(), "")
      || callOr(() => app.currentItem().artist(), "")
      || ""
    );
    return {
      title: title,
      artist: artist,
      duration_ms: Math.max(0, Math.round(Number(callOr(() => app.durationOfCurrentItem(), 0) || 0) * 1000)),
      position_ms: Math.max(0, Math.round(Number(callOr(() => app.playerPosition(), 0) || 0) * 1000)),
      is_playing: Boolean(callOr(() => app.playing(), false)),
      can_play_pause: true,
      can_next: true,
      can_previous: true
    };
  } catch (e) {
    return null;
  }
}

function parseYouTubeMusicTitle(windowTitle) {
  const marker = " | YouTube Music";
  if (!windowTitle || !windowTitle.endsWith(marker)) {
    return null;
  }
  const raw = windowTitle.slice(0, windowTitle.length - marker.length).trim();
  if (raw === "") {
    return null;
  }
  // Most YT Music titles are either "Track - Artist" or just "Track".
  const pieces = raw.split(" - ");
  const title = String((pieces[0] || "").trim());
  if (title === "") {
    return null;
  }
  const artist = pieces.length > 1 ? String(pieces.slice(1).join(" - ").trim()) : "";
  return {
    title: title,
    artist: artist,
    duration_ms: 0,
    position_ms: 0,
    is_playing: true,
    can_play_pause: false,
    can_next: false,
    can_previous: false
  };
}

function readFirefoxYouTubeMusic() {
  try {
    const app = Application("Firefox");
    if (!app.running()) {
      return null;
    }
    const titles = callOr(() => app.windows.name(), []);
    for (const title of titles) {
      const parsed = parseYouTubeMusicTitle(String(title || ""));
      if (parsed !== null) {
        return parsed;
      }
    }
    return null;
  } catch (e) {
    return null;
  }
}

let current = null;
const readers = [
  () => readMusicLike("Music"),
  () => readMusicLike("Spotify"),
  () => readMusicLike("TV"),
  () => readVlc(),
  () => readFirefoxYouTubeMusic(),
];
for (const read of readers) {
  const value = read();
  if (value !== null) {
    current = value;
    break;
  }
}

current === null ? "null" : JSON.stringify(current);
"#;

const EXECUTE_TOGGLE_SCRIPT: &str = r#"
function callOk(f) {
  try {
    f();
    return true;
  } catch (e) {
    return false;
  }
}

function sendToggleMusicLike(name) {
  try {
    const app = Application(name);
    if (!app.running()) {
      return false;
    }
    return callOk(() => app.playpause()) || callOk(() => app.playPause());
  } catch (e) {
    return false;
  }
}

function sendToggleVlc() {
  try {
    const app = Application("VLC");
    if (!app.running()) {
      return false;
    }
    let playing = false;
    try {
      playing = Boolean(app.playing());
    } catch (e) {
      playing = false;
    }
    if (playing) {
      return callOk(() => app.pause());
    }
    return callOk(() => app.play());
  } catch (e) {
    return false;
  }
}

sendToggleMusicLike("Music")
  || sendToggleMusicLike("Spotify")
  || sendToggleMusicLike("TV")
  || sendToggleVlc();
"#;

const EXECUTE_NEXT_SCRIPT: &str = r#"
function callOk(f) {
  try {
    f();
    return true;
  } catch (e) {
    return false;
  }
}

function sendNextMusicLike(name) {
  try {
    const app = Application(name);
    if (!app.running()) {
      return false;
    }
    return callOk(() => app.nextTrack()) || callOk(() => app.next());
  } catch (e) {
    return false;
  }
}

function sendNextVlc() {
  try {
    const app = Application("VLC");
    if (!app.running()) {
      return false;
    }
    return callOk(() => app.next()) || callOk(() => app.nextTrack());
  } catch (e) {
    return false;
  }
}
sendNextMusicLike("Music")
  || sendNextMusicLike("Spotify")
  || sendNextMusicLike("TV")
  || sendNextVlc();
"#;

const EXECUTE_PREVIOUS_SCRIPT: &str = r#"
function callOk(f) {
  try {
    f();
    return true;
  } catch (e) {
    return false;
  }
}

function sendPreviousMusicLike(name) {
  try {
    const app = Application(name);
    if (!app.running()) {
      return false;
    }
    return callOk(() => app.previousTrack()) || callOk(() => app.previous());
  } catch (e) {
    return false;
  }
}

function sendPreviousVlc() {
  try {
    const app = Application("VLC");
    if (!app.running()) {
      return false;
    }
    return callOk(() => app.previous()) || callOk(() => app.previousTrack());
  } catch (e) {
    return false;
  }
}
sendPreviousMusicLike("Music")
  || sendPreviousMusicLike("Spotify")
  || sendPreviousMusicLike("TV")
  || sendPreviousVlc();
"#;

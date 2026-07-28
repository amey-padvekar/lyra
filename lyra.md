# Lyra — MVP Spec (Rust)

A cross-platform, OS-level overlay that shows the track currently playing in your
browser (or any app) with time-synced lyrics. Not a browser extension — a native
always-on-top window that reads the OS media session. Built in Rust, optimized for a
small idle footprint since it runs continuously in the background.

---

## Goal of the MVP

Prove the full loop end to end on **one platform (Linux)**, with the smallest feature
set that is genuinely usable:

> While music plays in the browser, a bottom-right card shows the current track and
> the lyric line advances in sync. A global hotkey toggles it.

If that works, the other platforms and the polish are additive.

## Explicitly *not* in the MVP

- Windows and macOS backends (design the trait for them; implement later)
- Settings UI, themes, font pickers
- Album-art color theming, blur/glass effects
- Manual per-track offset tuning
- Packaging/installers, auto-update
- Multi-monitor awareness beyond "remember last position"

---

## Stack

| Concern             | Choice                              | Why |
|---------------------|-------------------------------------|-----|
| Language            | Rust                                | Lightest native footprint; strong lib ecosystem for this; chosen to learn |
| Overlay UI          | **Slint**                           | Declarative, built for "lightweight + smooth animations"; no webview |
| Windowing backend   | winit (under Slint)                 | Gives always-on-top / frameless / transparency knobs |
| Media read — Linux  | **`mpris`** crate (D-Bus/MPRIS2)    | Reads the active player's metadata + position; has a ProgressTracker for live info |
| Media read — Win    | `windows` crate → GSMTC             | GlobalSystemMediaTransportControlsSessionManager (post-MVP) |
| Media read — macOS  | MediaRemote bindings (objc2)        | Private framework; hardest backend (post-MVP) |
| HTTP (lyrics)       | **`ureq`** (blocking)               | No async runtime — keeps the binary lean vs reqwest+tokio |
| JSON                | `serde` + `serde_json`              | LRCLIB responses |
| LRC parsing         | small hand-rolled parser            | ~40 lines; no dependency needed |
| Global hotkey       | `global-hotkey` crate               | Cross-platform show/hide toggle |
| Cache path          | `directories` crate                 | Correct per-OS cache dir |
| Errors              | `anyhow` (app) / `thiserror` (lib)  | Standard split |

**Key correction from earlier planning:** `souvlaki` is *not* used. It exposes *your*
app as a media source to the OS (outbound); Lyra needs to *read* what the browser is
playing (inbound). There is no clean pure-Rust cross-platform reader, so reading is
done per-OS behind one trait — `mpris` on Linux, GSMTC on Windows, MediaRemote on macOS.

Overlay format: **bottom-right card** — album art, title/artist, current lyric line
with 1–2 dimmed lines above/below. Semi-opaque rounded background (not full
transparency), which also sidesteps flaky transparent-window behavior. Window is
always-on-top, frameless, click-through, draggable; last position remembered.

---

## Architecture

Five parts behind clean seams. The `MediaReader` trait is the platform boundary.

```
media reader (trait) ──▶ sync engine ──▶ Slint UI (card)
                             ▲
   lyrics fetcher ───────────┘   (LRCLIB via ureq + disk cache)

   global hotkey ───────────────▶ Slint UI (show/hide)
```

1. **Media reader** — a `MediaReader` trait; one impl per OS behind `#[cfg(target_os)]`.
   MVP ships the Linux/`mpris` impl only. Polls on its own thread; sends updates over
   a channel to the UI thread.
2. **Lyrics fetcher** — queries LRCLIB by title + artist + duration (`ureq` + `serde`),
   parses LRC into a sorted `Vec<LyricLine>`, caches to disk keyed by track.
3. **Sync engine** — pure, testable: interpolates position between polls, picks the
   active line, handles seek / pause / track-change / drift. No I/O.
4. **Overlay window** — a Slint `.slint` component for the card, driven by a Rust
   model; receives active-line updates.
5. **Toggle** — `global-hotkey` shows/hides the window regardless of focus.

### Core types

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub position_ms: u64,
    pub received_at: std::time::Instant, // when position_ms was read — for interpolation
}

pub trait MediaReader: Send {
    /// Current now-playing state, or None if nothing is playing.
    fn poll(&mut self) -> anyhow::Result<Option<NowPlaying>>;
}

#[derive(Clone, Debug)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}
```

### The sync model (must feel right)

- **Active line** = last line whose `time_ms <= interpolated_position`.
- **Interpolation**: while playing,
  `interpolated = last.position_ms + last.received_at.elapsed()`; frozen while paused.
  Recompute every UI frame, not every poll.
- **Seek detection**: if a fresh poll's position differs sharply from the interpolated
  guess, snap to the new line instead of animating through.
- **Drift correction**: each real poll, ease interpolated toward reported rather than
  hard-snapping.
- **Track change**: new title/artist → drop lyrics, re-query LRCLIB, reset timeline.
- **No synced lyrics**: fall back to the card with plain/static lyrics or no scroll.

### Threading

Slint owns the UI event loop. The media reader runs on a worker thread and posts
`NowPlaying` updates via a channel; apply them to the UI with a Slint weak handle /
`invoke_from_event_loop`. Lyrics fetches also run off the UI thread (blocking `ureq`
on a worker) so the card never stutters.

---

## LRCLIB usage

- `GET https://lrclib.net/api/get?track_name=...&artist_name=...&duration=...`
- Send a `User-Agent` naming the app (requested by LRCLIB; free, no key, no rate limit).
- Match on duration to disambiguate remixes / live versions.
- Cache each hit to disk; check cache before the network.
- Response has `syncedLyrics` (LRC, timestamped) and `plainLyrics` (fallback).

---

## Build order (each step de-risks the next)

1. **Sync engine + LRC parser, headless.** Pure module with `cargo test`: parse a real
   `.lrc`, feed a simulated advancing clock, assert the active line changes at the right
   times. No UI, no platform, no network. This validates the timing model in isolation
   and is a gentle first Rust milestone.
2. **Linux media reader (`mpris`).** Print live `{title, artist, position}` from the
   browser to stdout. The make-or-break platform spike — easy on Linux via MPRIS.
3. **Wire 1 + 2 + LRCLIB.** Real track in → correct lyric line printed in sync to
   stdout. Full loop, still no UI.
4. **Slint card.** Replace stdout with the bottom-right card; wire the model + channel.
5. **`global-hotkey` toggle**, then polish: line transitions (Slint animations),
   drift smoothing, remembered position, "no lyrics" fallback.

MVP is "done" at the end of step 5 on Linux.

---

## After the MVP

- **Windows** backend: GSMTC via `windows-rs` (verbose async WinRT — budget time).
- **macOS** backend: MediaRemote via `objc2` bindings (private framework — hardest).
- Settings: font, size, opacity, position presets, per-track offset nudge.
- Visual polish: album-art color theming, fade in/out.
- Packaging per OS.

---

## Open risks to watch

- **Click-through + transparent + always-on-top window** in Slint/winit behaves
  differently per OS (`set_cursor_hittest`, window level, decorations). Verify in the
  spike — it's load-bearing for the UX. The semi-opaque card lowers the risk.
- **macOS MediaRemote is a private API** — fine for a personal/portfolio project, can't
  ship to the App Store, and Apple has broken it before.
- **Windows GSMTC via windows-rs** is verbose WinRT async — expect friction.
- **Position freshness** varies by source; interpolation covers it but expect drift on
  scrub.
- **Lyrics coverage** isn't 100%; the plain/no-lyrics fallback must be graceful.
- **Rust learning curve** — ownership/borrowing and threading against Slint's event
  loop are the two spots most likely to slow you early; steps 1–3 keep you away from
  the hardest parts until you've got momentum.
- **Legal**: lyrics are copyrighted and LRCLIB is a community gray area — personal use,
  don't commercialize.
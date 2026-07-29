# Lyra

A native, always-on-top overlay that shows whatever is currently playing on your
machine — with time-synced lyrics that advance line by line.

Not a browser extension. Lyra reads the OS-level media session, so it works with
whatever is actually playing: a browser tab, a desktop player, anything that
registers with the system transport controls.

```
┌──────────────────────────────────────────┐
│              Song Title                  │
│                Artist                    │
│                                          │
│      the current lyric line, in sync     │
│                                          │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░  │
│  1:47                              3:24  │
│              ⏮   ⏸   ⏭                   │
└──────────────────────────────────────────┘
```

## Status

Early, but functional end to end. **Windows only** right now.

The original spec (`lyra.md`) targeted Linux/MPRIS first; that got pivoted to
Windows/GSMTC because `mpris` needs D-Bus and pkg-config, which don't exist on
the development machine. The `MediaReader` trait boundary is designed for all
three platforms, but `src/media/linux.rs` and `src/media/macos.rs` are still
empty stubs — they compile, they don't do anything.

## Features

- **Now playing** — title and artist from the system media session
- **Synced lyrics** — the active line, interpolated between polls so it advances
  smoothly rather than jumping once a second
- **Seek bar** — progress plus elapsed / total time
- **Media controls** — previous, play/pause, next. Buttons grey out when the
  source says it won't honour them
- **Global hotkey** — `Ctrl+Shift+L` shows/hides the card from anywhere,
  regardless of focus
- **Frameless, always-on-top, translucent card** — draggable, and it remembers
  where you left it

## Requirements

- Windows 10/11
- Rust (edition 2024 — needs a reasonably current toolchain)

## Build and run

```sh
cargo run          # build and launch
cargo build --release
cargo test         # unit tests (sync engine + LRC parser)
cargo check        # fast type-check
cargo clippy       # lint
```

First build pulls in Slint's rendering stack and the `windows` crate, so expect
it to take a while and want a few GB of disk.

## Usage

Launch it and it picks up whatever is playing. Drag the card anywhere; the
position is saved. `Ctrl+Shift+L` toggles visibility.

**To quit, kill the process** — there's no close button or quit hotkey yet (see
Limitations).

## How it works

```
media reader (trait) ──▶ sync engine ──▶ Slint UI (card)
        ▲                    ▲
        │                    │
   transport            lyrics fetcher
   commands             (LRCLIB → lyrics.ovh, disk-cached)

   global hotkey ─────────▶ show / hide
```

Three threads:

- **Media thread** polls the session once a second and pushes `NowPlaying` over
  a channel. It also executes transport commands, because the underlying COM
  handles are `!Send` and can't be touched from anywhere else. Its wait is a
  `recv_timeout`, so a button press acts immediately instead of waiting out the
  poll interval.
- **Lyrics thread** does the blocking HTTP lookups. Kept off the UI thread so a
  fetch on track change can't stutter the card.
- **UI thread** owns the Slint event loop and runs a 66ms timer that drains both
  channels and repaints.

The interesting piece is `src/sync/engine.rs` — it's pure, no I/O, and unit
tested. It interpolates position between polls, snaps on a seek, eases small
drift rather than jerking, and freezes while paused.

Two non-obvious things it gets right, both of which caused real bugs:

- Track identity is `(title, artist)` **without** duration. Some sources report
  a duration that jitters by a few milliseconds between polls, which otherwise
  reads as a new track on every single poll.
- Position is anchored to the session's own `LastUpdatedTime`, not to when we
  happened to poll. `Position()` is only accurate as of that timestamp, and
  using poll time makes interpolation drift against real playback.

## Lyrics

1. **[LRCLIB](https://lrclib.net)** — free, no API key, and the only good source
   of *timestamped* lyrics. Matched on title + artist + duration.
2. **[lyrics.ovh](https://lyrics.ovh)** — fallback when LRCLIB has no match.
   Plain text only, so it renders as a static block with no line syncing.

Results are cached to disk before any network call, keyed by a hash of
`(title, artist, duration)` — hashed rather than using the names directly
because Windows forbids `/ : ? * " < > |` in filenames and real track titles
contain them.

Cache lives in `%LOCALAPPDATA%\lyra\lyra\cache\`, alongside the saved window
position. Deleting it is safe.

## Limitations

- **Windows only.** Linux and macOS readers are unimplemented stubs.
- **No clean quit.** Frameless window, no tray icon, no quit hotkey — you have
  to kill the process.
- **Not click-through.** `lyra.md` wanted it; Slint 1.x doesn't expose winit's
  `set_cursor_hittest`, and full click-through would conflict with the drag and
  button handling anyway.
- **Skip has visible lag.** Some sources take ~2 seconds to republish metadata
  after a track change, so the card trails briefly. That's the source, not Lyra.
- **Lyrics coverage isn't complete.** Plenty of tracks have no synced lyrics
  anywhere; regional and independent releases especially.
- **No settings.** Font, size, opacity, and offset nudging are all hardcoded.

## Development

Unit tests cover the sync engine and LRC parser:

```sh
cargo test -p lyra --lib
```

`examples/` holds throwaway probes used to verify each piece against live state
— useful when something breaks and you want to isolate which layer:

```sh
cargo run --example gsmtc_probe          # what the media session reports
cargo run --example media_control_probe  # transport commands (controls playback)
cargo run --example lrclib_probe         # lyrics lookup + fallback
cargo run --example lyrics_ovh_probe     # the plain-text fallback on its own
cargo run --example lyrics_cache_probe   # disk cache round-trip
```

Further docs: `lyra.md` is the original design spec and the reasoning behind
each dependency choice; `IMPLEMENTATION_PLAN.md` is the concrete build order
that was actually followed.

## Legal

Lyrics are copyrighted, and LRCLIB is a community-run gray area. This is a
personal project — keep it personal, don't commercialize it.

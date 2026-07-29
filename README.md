# Lyra

Lyra is a native Windows overlay that shows the currently playing media and
syncs a lyric line with playback in real time. It reads the system media session,
so it works with browsers, desktop players, and other apps that expose transport
controls.

## What it does

Lyra watches the active media session, keeps a small always-on-top card in sync
with playback, and gives you a simple way to control the current source. When a
track changes, it fetches lyrics, uses cached results when available, and updates
what is shown on the overlay without blocking the UI.

## Current status

Lyra is still an early-stage project, but the core pieces are already wired up in
this codebase:

- a Slint-based overlay window
- Windows media polling through GSMTC
- transport controls for play/pause, previous, and next when the source supports them
- lyric lookup through LRCLIB with a fallback to lyrics.ovh
- local disk caching for lyrics
- a global hotkey to show or hide the overlay
- a tray menu for show/hide and quit actions

The remaining work is mostly around polish, packaging, and expanding platform
support. Linux and macOS media backends are still not implemented.

## Features

- Show the current track title, artist, and lyric line in a compact overlay
- Keep the lyric line moving smoothly as playback progresses
- Display elapsed and total time with a progress indicator
- Send previous, play/pause, and next commands when the source supports them
- Fetch lyrics from LRCLIB and fall back to lyrics.ovh when needed
- Cache lyrics locally so repeated lookups are faster
- Keep the overlay above other windows and remember its last position and size
- Toggle the overlay from anywhere with Ctrl+Shift+L
- Show or hide the app and quit it from the system tray

## How it works

1. A media reader polls the current session and publishes the latest state.
2. A sync engine estimates the current playback position and chooses the active lyric line.
3. A background lyrics worker loads cached lyrics or fetches new ones from LRCLIB or lyrics.ovh.
4. The Slint UI updates the overlay with track info, lyrics, progress, and transport controls.

The main pieces of the project are organized around these areas:

- src/media for platform-specific media readers
- src/sync for playback and lyric synchronization logic
- src/lyrics for fetching, parsing, and caching lyrics
- src/overlay for the card UI and window behavior

## Building on Windows

### Prerequisites

- Windows 10 or 11
- a Rust 2024-capable toolchain from rustup
- Visual Studio Build Tools with the Desktop development with C++ workload

Check that the toolchain is available:

```sh
rustc --version && cargo --version
```

### Development build

```sh
cargo run
cargo test
cargo check
cargo clippy
```

### Release build

```sh
cargo build --release
```

The release binary is written to target/release/lyra.exe.

### Notes

- The first build can take a while because it compiles the Slint stack and the Windows crates.
- If you hit memory issues during the build, try:

```sh
cargo build -j 1
```

- A stale target directory can also cause build problems; cargo clean is a good fallback if you see crate artifact issues.

## Usage

Launch the app and it will begin tracking whatever is playing. Drag the card to
move it, resize it from any edge, and use the tray menu or Ctrl+Shift+L to show
or hide it.

## Logs and debugging

The app writes logs to the local app data directory for Lyra, which is the best
place to look when something goes wrong. The logs are especially useful for
reporting bugs because release builds do not keep a console open.

## Legal

Lyrics are copyrighted, and LRCLIB is a community-run service with a gray legal
status. This project is intended for personal use only.

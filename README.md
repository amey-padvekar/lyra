# Lyra

Lyra is a native desktop overlay that shows the currently playing media and
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
- macOS media polling through the system Now Playing session (via `media-control`,
  when installed) with an AppleScript fallback (Music, Spotify, TV, and VLC)
- transport controls for play/pause, previous, and next when the source supports them
- click-to-seek on the progress bar, when the source reports it allows it
- lyric lookup through LRCLIB with a fallback to lyrics.ovh
- local disk caching for lyrics
- a global hotkey to show or hide the overlay
- a tray/menu-bar menu for show/hide and quit actions

The remaining work is mostly around polish, packaging, and expanding platform
support. Linux media backend is still not implemented, and macOS support is
currently limited to AppleScript-accessible players (Music, Spotify, TV, VLC).

## Features

- Show the current track title, artist, and lyric line in a compact overlay
- Keep the lyric line moving smoothly as playback progresses
- Display elapsed and total time with a progress indicator
- Send previous, play/pause, and next commands when the source supports them
- Click the progress bar to jump to that point in the track
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

### Release build, step by step

1. Open PowerShell in the project root.
2. Make sure Rust is installed and available on `PATH`.
3. If you want a binary that does not depend on the Visual C++ redistributable, set static CRT before building:

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
```

4. Build the release binary:

```sh
cargo build --release
```

5. Wait for the build to finish. The executable will be created at `target/release/lyra.exe`.
6. Run the binary from `target/release/lyra.exe` or copy it somewhere else for distribution.

If you do not need a fully portable binary, you can skip step 3 and run only step 4.

### Development build

```sh
cargo run
cargo test
cargo check
cargo clippy
```

### Notes

- The first build can take a while because it compiles the Slint stack and the Windows crates.
- If you hit memory issues during the build, try:

```sh
cargo build -j 1
```

- A stale target directory can also cause build problems; cargo clean is a good fallback if you see crate artifact issues.

## Building on macOS

### Prerequisites

- macOS 13+
- a Rust 2024-capable toolchain from rustup
- Apple Music and/or Spotify installed if you want live media polling
- Optional but recommended: `brew install media-control` — reads macOS's
  system-level Now Playing session, so Lyra tracks whatever the OS considers
  "now playing" regardless of which app or browser tab currently has focus
  (including a backgrounded browser tab). Without it, Lyra falls back to
  AppleScript automation (see notes below).

### Development build

```sh
cargo run
```

### Release build

```sh
cargo build --release
```

The binary is created at `target/release/lyra` (an arm64 or x86_64 Mach-O
executable depending on your Mac). Run it directly:

```sh
./target/release/lyra
```

or copy it somewhere on your `PATH` (e.g. `/usr/local/bin`) to launch it by
name. This is a bare binary, not a `.app` bundle — no icon, no Dock/Spotlight
entry, no code signing. It's meant for running on your own Mac from a
terminal or a launch script, not for handing to someone else; macOS
Gatekeeper will warn on an unsigned binary from another machine.

### Notes

- With `media-control` installed, the macOS backend reads the system Now
  Playing session, which works the same whether the source app/tab is
  focused or in the background, and reports real elapsed time and duration
  for browser-based sources like YouTube Music. This works by shelling out to
  the `media-control` CLI, which itself works around Apple's post-macOS-15.4
  lockdown of the private `MediaRemote.framework` by using a signed system
  binary that's still allow-listed — see
  [ungive/media-control](https://github.com/ungive/media-control) for how it
  works. Since this rides on a private framework one level removed, Apple
  could break it again in a future release; Lyra falls back to AppleScript
  automatically if `media-control` stops working or isn't installed.
- Without `media-control`, the macOS backend uses `osascript` (JavaScript for
  Automation) against Music, Spotify, TV, and VLC only. On first use, macOS
  may prompt for Automation permissions so Lyra can control them. Firefox +
  YouTube Music is detected only via a window-title fallback in this mode —
  it requires the YouTube Music tab to be focused, timeline position/duration
  are unavailable, and transport controls don't reach the browser at all.
- If nothing recognized is playing, Lyra reports no active media session.
- The overlay is configured to stay visible across desktop Spaces, including fullscreen app Spaces.

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

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

Lyra is in **early scaffolding**. The module tree in `src/` mirrors the target
architecture (see below), but almost every type is currently an empty struct or a
stub function (e.g. `pub struct SyncEngine;`, `parse_lrc` returns `Vec::new()`,
`main.rs` just prints `"Lyra"`). `ui/main.slint` is a bare `Window` with no content
and is **not yet wired into the build** — there is no `build.rs` and no
`slint-build`/`slint-interpreter` build-dependency in `Cargo.toml` yet, so the Slint
UI does not actually compile into the binary today. When implementing UI work, that
build-time codegen wiring needs to be added first.

The full design intent — including the *why* behind each dependency choice, the
sync-engine timing model, and the intended build order — lives in **`lyra.md`** at
the repo root. Read it before implementing any module; it is the spec this codebase
is scaffolded from, not just background reading.

## Commands

```sh
cargo build              # build the project
cargo run                # run the binary (currently just prints "Lyra")
cargo test                # run all tests
cargo test <name>         # run a single test by name (substring match)
cargo test -p lyra --lib  # run only unit tests in src/ (no integration tests exist yet)
cargo check               # fast type-check without codegen
cargo clippy               # lint
```

There is no `Cargo.lock` committed yet and no CI config in the repo.

## Architecture

Five parts behind clean seams, connected by channels and a trait boundary
(full detail in `lyra.md`):

```
media reader (trait) ──▶ sync engine ──▶ Slint UI (card)
                             ▲
   lyrics fetcher ───────────┘   (LRCLIB via ureq + disk cache)

   global hotkey ───────────────▶ Slint UI (show/hide)
```

- **`src/domain.rs`** — shared core types (`NowPlaying`). This is the data contract
  between the media layer and the sync engine.
- **`src/media/`** — the `MediaReader` trait (`poll() -> anyhow::Result<Option<NowPlaying>>`)
  plus one implementation per OS, gated by `#[cfg(target_os)]`: `linux.rs` (MPRIS via
  the `mpris` crate — the only backend targeted for the MVP), `windows.rs` (GSMTC,
  post-MVP), `macos.rs` (MediaRemote via objc2, post-MVP, private-API risk). Each
  reader polls on its own thread and pushes updates over a channel — no reader may
  block the UI thread.
- **`src/sync/engine.rs`** — the `SyncEngine`: pure, no I/O, unit-testable in
  isolation. Interpolates playback position between polls
  (`interpolated = last.position_ms + last.received_at.elapsed()`), picks the active
  lyric line (last line with `time_ms <= interpolated_position`), and handles seek
  detection (snap on a sharp jump) vs. drift correction (ease toward reported
  position on a normal poll). This is the trickiest piece of logic in the project —
  see "The sync model" in `lyra.md` for the exact rules before touching it.
- **`src/lyrics/`** — `fetcher.rs` queries LRCLIB (`GET lrclib.net/api/get`) via
  `ureq`, matching on title + artist + duration; `parser.rs` is a small hand-rolled
  LRC parser (intentionally no parsing crate) producing `Vec<LyricLine>` sorted by
  `time_ms`; `cache.rs` persists fetched lyrics to disk (via `directories`), checked
  before any network call.
- **`src/overlay/`** — the Slint-backed always-on-top card window (bottom-right,
  semi-opaque, frameless, click-through, draggable, remembers last position). Driven
  from Rust via a Slint weak handle / `invoke_from_event_loop` so cross-thread
  updates land safely on the UI event loop.
- **`src/hotkey/`** — global show/hide toggle via the `global-hotkey` crate,
  independent of window focus.
- **`src/app.rs`** — intended wiring point that assembles the above into the running
  app; currently an empty `run()`.

### Threading model

Slint owns the UI event loop on the main thread. The media reader and the lyrics
fetcher (blocking `ureq`) each run on their own worker thread and communicate with
the UI via channels — never call into Slint UI state directly from a worker thread.

### Recommended build/implementation order

Per `lyra.md`, implement in this sequence so each step de-risks the next: (1) sync
engine + LRC parser as pure, headless, `cargo test`-able code; (2) the Linux MPRIS
reader, validated by printing to stdout; (3) wire reader + sync engine + LRCLIB
together, still stdout-only; (4) replace stdout with the Slint card; (5) add the
global hotkey toggle and polish. The MVP targets **Linux only** — Windows/macOS
media backends are designed for (the trait boundary) but not implemented.

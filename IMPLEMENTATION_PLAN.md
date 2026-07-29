# Lyra — Windows/GSMTC MVP Implementation Plan

Companion doc to `lyra.md` (the original design spec). This is the concrete,
ordered build plan actually being followed, including one deviation from
`lyra.md`: the MVP target platform is **Windows**, not Linux.

## Context

Lyra started as a scaffold matching `lyra.md`'s architecture but with every module
stubbed out (empty structs, `parse_lrc` returning `Vec::new()`, `main.rs` printing
`"Lyra"`). The spec's MVP targets Linux via the `mpris` crate, but the dev machine
here is Windows, and `mpris` was an *unconditional* dependency — `cargo build`
failed outright because `libdbus-sys` needs D-Bus/pkg-config, which don't exist on
Windows. Decision: **pivot the MVP target platform to Windows**, reading media state
via GSMTC (`GlobalSystemMediaTransportControlsSessionManager`, through the `windows`
crate) instead of MPRIS, so the app can actually be built and tested here. `mpris`/
Linux and MediaRemote/macOS become "designed for via the `MediaReader` trait,
implemented later" — the same role Windows/macOS already played in the original
Linux-first plan, just swapped.

Each step below has a hard verification gate — don't move to the next step until
the current one's gate passes for real (build/test/run it), per this project's own
build-order philosophy of de-risking incrementally.

Two numeric constants (`SEEK_THRESHOLD_MS`, `DRIFT_EASE_ALPHA`) are **proposed
defaults, not spec values** — `lyra.md` states the rules but not the numbers.

---

## Step 0 — Cargo.toml + module gating (unblocks everything else) — ✅ DONE

- `Cargo.toml`: `mpris = "2"` moved under `[target.'cfg(target_os = "linux")'.dependencies]`.
  Added `[target.'cfg(target_os = "windows")'.dependencies] windows = "0.62"` with
  `features = ["Media_Control"]`. Added `[build-dependencies] slint-build = "1"`.
- `build.rs` (new, repo root): `slint_build::compile("ui/main.slint").unwrap();`
- `src/media/mod.rs`: platform submodules gated with `#[cfg(target_os = "...")]`,
  plus `#[cfg(target_os = "windows")] pub use windows::GsmtcReader as PlatformReader;`
  so `app.rs` never needs its own `#[cfg]` blocks to pick a reader.

**Gate passed**: `cargo check` is green on this machine — `windows = "0.62"` with
`Media_Control` and `slint-build` both resolved without conflict.

---

## Step 1 — Sync engine + LRC parser, headless (`cargo test`, no platform, no network)

- **`src/domain.rs`**: add `received_at: std::time::Instant` to `NowPlaying` (per
  `lyra.md`'s spec version). **Drop `#[derive(PartialEq)]`** — with `Instant` in the
  struct, two polls of an unchanged track never compare equal. Add
  `fn track_id(&self) -> (&str, &str, u64)` returning `(title, artist, duration_ms)`.

- **`src/sync/engine.rs`** — `SyncEngine` fields: `lines: Vec<LyricLine>`,
  `current_track: Option<(String, String, u64)>`, `last_poll: Option<NowPlaying>`,
  `baseline_ms: u64`.
  - `set_lyrics(&mut self, lines: Vec<LyricLine>)` — **only caller controls this; do
    NOT auto-clear lines on track-change inside the engine.**
  - `on_poll(&mut self, np: NowPlaying, now: Instant)`: track-change via plain tuple
    comparison (`current_track.as_ref() != Some(&(...))` — tuples don't implement
    `Deref`, so `as_deref()` won't compile). On change: reset `baseline_ms`, update
    `current_track`, **do not touch `self.lines`** (app.rs owns lyrics lifecycle).
    On same track: `guess = interpolate(now)` using the *old* baseline;
    `diff = np.position_ms as i64 - guess as i64`; if `diff.abs() > SEEK_THRESHOLD_MS`
    (proposed `1000`) snap; else ease with `DRIFT_EASE_ALPHA` (proposed `0.2`). Use
    `saturating_*` arithmetic throughout.
  - `tick(&mut self, now: Instant) -> Option<String>` — returns the active line's
    **owned text** (not `&LyricLine` — avoids a lifetime hazard once this is called
    through `RefCell::borrow_mut()` in Step 4).
  - `interpolate(&self, now: Instant) -> u64` — frozen while paused; else
    `baseline_ms + elapsed`, clamped to `duration_ms`.

  **Bug avoided**: an earlier draft had `on_poll`'s track-change branch call
  `self.lines.clear()`. If `app.rs` calls `set_lyrics(fetched)` then `on_poll(np)`
  (the natural order), `on_poll` would wipe the lines it was just given, and no
  re-fetch would happen on later polls of the same track — lyrics silently never
  display again for that track. Keeping lyrics ownership solely in `app.rs` avoids
  this double-owner bug.

- **`src/lyrics/parser.rs`** — `parse_lrc`: match `[mm:ss.xx]text`, 2-digit frac =
  centiseconds ×10, 3-digit = ms; sort by `time_ms`. Non-timestamp metadata lines
  fail the numeric parse and are filtered out for free.

- **`tests/fixtures/lyrics/sample.lrc`** (new): 5-line LRC fixture with metadata
  tags plus timed lines spanning ~15s.

- **`cargo test` cases**: parse fixture (5 lines, sorted, metadata stripped); tick
  advances active line against a simulated clock (only ever build forward `Instant`s,
  never subtract); freezes while paused; snaps on a >1000ms jump; eases a small
  (~40ms) drift to land strictly between old/new guesses; resets timeline (not
  lines) on track change.

---

## Step 2 — Windows GSMTC reader

`src/media/windows.rs`: `GsmtcReader` (zero-field marker struct) implements
`MediaReader::poll()`:
1. `SessionManager::RequestAsync()` → poll `.Status()` in a busy-wait loop →
   `.GetResults()`.
2. `manager.GetCurrentSession()` — `Err` → `Ok(None)` ("nothing playing"). Unverified
   from static reading whether idle actually returns `Err` vs an `Ok`-wrapped null
   pointer — add a defensive check once tested live.
3. `session.TryGetMediaPropertiesAsync()` (same async-poll pattern) → `Title()`/`Artist()`.
4. `session.GetTimelineProperties()` (sync) → `Position()`/`StartTime()`/`EndTime()`
   are 100ns-tick `TimeSpan`s → `ms = ticks / 10_000`.
5. `session.GetPlaybackInfo()` (sync) → `PlaybackStatus() == Playing`.

**Why zero-field**: no explicit `Send`/`Sync` impl exists for the generated COM
interface wrapper types in `windows-core`, so they're likely `!Send`. Since
`MediaReader: Send` is required, never store a COM handle across `poll()` calls.

**Live-only unknowns**: COM apartment init (`CO_E_NOTINITIALIZED` / `0x800401F0` →
`CoInitializeEx` + `Win32_System_Com` feature) and whether `Media_Control` alone is
a sufficient feature set.

**Gate**: a throwaway probe that polls `GsmtcReader` every second and prints
`{title, artist, position}` while music plays in a browser.

---

## Step 3 — Wire reader + sync engine + LRCLIB (stdout, no UI yet)

- **`src/cache/mod.rs`**: `AppCache` = thin disk-cache-path helper via
  `directories::ProjectDirs`.
- **`src/lyrics/cache.rs`**: `LyricsCache` composes `AppCache`, keyed by **hashing**
  `(title, artist, duration_ms)` (Windows forbids `/ : ? * " < > |` in filenames,
  which real track/artist names can contain).
- **`src/lyrics/fetcher.rs`**: `LyricsFetcher::fetch(...)` — `ureq` GET to
  `https://lrclib.net/api/get` with a `User-Agent`; prefer `syncedLyrics`, fall back
  to `plainLyrics` as a single `time_ms: 0` line.
- **`src/app.rs`** `run()`: reader on its own thread → `mpsc::channel`. Main loop
  owns lyrics lifecycle: on track change, clear engine lyrics immediately, check
  cache before fetching, then `set_lyrics`. Always call `on_poll`. Print `tick()`
  output to stdout on change.

**Gate**: one real `https://lrclib.net/api/get` round-trip succeeds and deserializes.

---

## Step 4 — Slint card

- **`ui/main.slint`**: `in property <string> track-title/track-artist/current-line;`
  in a simple `VerticalLayout`. Always-on-top/frameless/click-through/draggable
  window flags are `lyra.md`'s top project-wide risk — this is the step to spike it.
- **`src/overlay/mod.rs`**: `slint::include_modules!()` generates `MainWindow`;
  `OverlayWindow` wraps it with `as_weak()`/`run()`.
- **`src/app.rs`**: replace the stdout loop with a `slint::Timer` (~66ms) doing the
  same logic, pushing into the UI via `weak.upgrade()` — safe directly since a
  `Timer` callback already runs on the UI thread.

**Gate**: app runs, card renders and updates live against real playback; window
flags behave as intended on Windows.

---

## Step 5 — Global hotkey + polish

- **`src/hotkey/mod.rs`**: `HotkeyManager` holds a `GlobalHotKeyManager` (must stay
  alive) and registers a toggle hotkey.
- **`src/app.rs`**: same timer tick, non-blockingly drain
  `GlobalHotKeyEvent::receiver().try_recv()`, toggle visibility.
- Polish: line-transition animation, remembered window position (via `AppCache`),
  "no lyrics" fallback (already covered by Step 3's plain-lyrics/empty-vec handling).

**Gate**: hotkey toggles the window regardless of focus — MVP done per `lyra.md`.

---

## Flagged-unresolved (live verification needed, not resolved by more reading)

1. `GetCurrentSession()` idle-system behavior (`Err` vs `Ok`-wrapped null) — Step 2.
2. COM apartment init on the reader thread (`0x800401F0` → `CoInitializeEx`) — Step 2.
3. Always-on-top/click-through/frameless Slint window flags on Windows — Step 4.
4. `windows` crate version actually resolved by Cargo — resolved: `0.62`, confirmed
   via `cargo check` in Step 0.
  
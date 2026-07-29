use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use crate::domain::NowPlaying;
use crate::hotkey::HotkeyManager;
use crate::lyrics::cache::LyricsCache;
use crate::lyrics::fetcher::LyricsFetcher;
use crate::lyrics::LyricLine;
use crate::media::{MediaCommand, MediaReader, PlatformReader};
use crate::overlay::OverlayWindow;
use crate::sync::engine::SyncEngine;

/// How long the outgoing line's fade-out takes before the text swaps and the
/// incoming line starts fading in — kept equal to `main.slint`'s `animate
/// opacity` duration so the swap lands exactly at the invisible trough.
const LINE_FADE_MS: u64 = 150;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

type TrackId = (String, String, u64);

/// Identity is (title, artist) only, not duration_ms: some GSMTC sources
/// (notably browser-based players) report a duration that jitters by a few ms
/// poll-to-poll, which would otherwise look like a new track on every poll —
/// clearing lyrics continuously and discarding every fetch response before it
/// ever lands, because by the time it arrives `current_track` has already
/// "changed" again.
fn same_track(a: &TrackId, b: &TrackId) -> bool {
    a.0 == b.0 && a.1 == b.1
}

fn format_mmss(ms: u64) -> String {
    let total_secs = ms / 1_000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes}:{seconds:02}")
}

pub fn run() {
    let (np_tx, np_rx) = mpsc::channel::<NowPlaying>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<MediaCommand>();
    let (lyrics_req_tx, lyrics_req_rx) = mpsc::channel::<TrackId>();
    let (lyrics_res_tx, lyrics_res_rx) = mpsc::channel::<(TrackId, Vec<LyricLine>)>();

    // Transport commands run on this same thread rather than the UI thread:
    // the GSMTC handles are `!Send`, so they can't cross threads, and issuing
    // one blocks on a WinRT round-trip that would stutter the card.
    thread::spawn(move || {
        let mut reader = PlatformReader;
        loop {
            match reader.poll() {
                Ok(Some(np)) => {
                    if np_tx.send(np).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => eprintln!("media poll error: {e}"),
            }

            // Doubles as the poll interval, but returns the instant a command
            // arrives — so a button press acts immediately instead of waiting
            // out the rest of the tick, and the poll right after it refreshes
            // the card with the result.
            match cmd_rx.recv_timeout(POLL_INTERVAL) {
                Ok(command) => {
                    if let Err(e) = reader.execute(command) {
                        eprintln!("media command {command:?} failed: {e}");
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Lyrics fetches block on network I/O, so they run on their own worker
    // thread — a Slint `Timer` callback runs on the UI thread, and blocking it
    // for an HTTP round-trip would freeze the card on every track change.
    thread::spawn(move || {
        let cache = match LyricsCache::new() {
            Ok(cache) => cache,
            Err(e) => {
                eprintln!("could not open lyrics cache: {e}");
                return;
            }
        };

        for (title, artist, duration_ms) in lyrics_req_rx {
            let lines = match cache.get(&title, &artist, duration_ms) {
                Some(lines) => {
                    eprintln!(
                        "lyrics: cache hit for {title} - {artist}: {} lines",
                        lines.len()
                    );
                    lines
                }
                None => match LyricsFetcher::fetch(&title, &artist, duration_ms) {
                    Ok(lines) => {
                        eprintln!(
                            "lyrics: fetched {title} - {artist} ({duration_ms}ms): {} lines",
                            lines.len()
                        );
                        if let Err(e) = cache.set(&title, &artist, duration_ms, &lines) {
                            eprintln!("could not cache lyrics: {e}");
                        }
                        lines
                    }
                    Err(e) => {
                        eprintln!("lyrics: fetch failed for {title} - {artist}: {e}");
                        Vec::new()
                    }
                },
            };
            if lyrics_res_tx.send(((title, artist, duration_ms), lines)).is_err() {
                break;
            }
        }
    });

    let overlay = match OverlayWindow::new() {
        Ok(overlay) => overlay,
        Err(e) => {
            eprintln!("could not create overlay window: {e}");
            return;
        }
    };
    let weak = overlay.as_weak();

    overlay.on_media_command(move |command| {
        if cmd_tx.send(command).is_err() {
            eprintln!("media thread is gone, dropping {command:?}");
        }
    });

    // Must be created on this (the UI event loop's) thread: it creates a
    // hidden window that only receives WM_HOTKEY messages while this thread's
    // message loop is being pumped, which starts below via `overlay.run()`.
    let hotkeys = match HotkeyManager::new() {
        Ok(hotkeys) => Some(hotkeys),
        Err(e) => {
            eprintln!("could not register global hotkey, toggle disabled: {e}");
            None
        }
    };

    let mut engine = SyncEngine::new();
    let mut current_track: Option<TrackId> = None;
    let mut last_line: Option<String> = None;

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(66),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };

            if hotkeys.as_ref().is_some_and(HotkeyManager::poll_toggle) {
                let window = ui.window();
                let toggled = if window.is_visible() {
                    window.hide()
                } else {
                    window.show()
                };
                if let Err(e) = toggled {
                    eprintln!("could not toggle overlay visibility: {e}");
                }
            }

            while let Ok(np) = np_rx.try_recv() {
                let track_id = (np.title.clone(), np.artist.clone(), np.duration_ms);
                let changed = match &current_track {
                    Some(current) => !same_track(current, &track_id),
                    None => true,
                };
                if changed {
                    eprintln!(
                        "track changed: {} - {} ({}ms)",
                        track_id.0, track_id.1, track_id.2
                    );
                    engine.set_lyrics(Vec::new());
                    ui.set_track_title(np.title.clone().into());
                    ui.set_track_artist(np.artist.clone().into());
                    let _ = lyrics_req_tx.send(track_id.clone());
                    current_track = Some(track_id);
                }

                // Refreshed every poll, not just on track change: play state
                // and which transports the source honours can both change
                // mid-track.
                ui.set_is_playing(np.is_playing);
                ui.set_can_play_pause(np.can_play_pause);
                ui.set_can_next(np.can_next);
                ui.set_can_previous(np.can_previous);

                engine.on_poll(np);
            }

            while let Ok((track_id, lines)) = lyrics_res_rx.try_recv() {
                match &current_track {
                    Some(current) if same_track(current, &track_id) => {
                        eprintln!("lyrics: applying {} lines to engine", lines.len());
                        engine.set_lyrics(lines);
                    }
                    _ => {
                        eprintln!(
                            "lyrics: discarding stale response for {} - {}",
                            track_id.0, track_id.1
                        );
                    }
                }
            }

            let now = Instant::now();

            let line = engine.tick(now).unwrap_or_default();
            if last_line.as_ref() != Some(&line) {
                last_line = Some(line.clone());

                // Fade the outgoing line out; the text swap and fade-in are
                // deferred to a one-shot timer landing at the fade's
                // invisible trough, so the swap itself is never seen.
                ui.set_line_opacity(0.0);
                let weak_for_swap = weak.clone();
                slint::Timer::single_shot(Duration::from_millis(LINE_FADE_MS), move || {
                    let Some(ui) = weak_for_swap.upgrade() else {
                        return;
                    };
                    ui.set_current_line(line.into());
                    ui.set_line_opacity(1.0);
                });
            }

            let position_ms = engine.position_ms(now);
            let duration_ms = engine.duration_ms();
            let progress = if duration_ms > 0 {
                (position_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ui.set_progress(progress);
            ui.set_elapsed_label(format_mmss(position_ms).into());
            ui.set_total_label(format_mmss(duration_ms).into());
        },
    );

    if let Err(e) = overlay.run() {
        eprintln!("overlay window error: {e}");
    }
}

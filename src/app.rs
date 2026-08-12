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
use crate::overlay::{MainWindow, OverlayWindow};
use crate::sync::engine::SyncEngine;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use crate::tray::{Tray, TrayCommand};

/// `LyricRow` role values — kept in sync with the `role` contract documented
/// on `LyricRow` in `main.slint`.
const ROLE_CURRENT: i32 = 0;
const ROLE_NEXT: i32 = 1;
const ROLE_ENTERING: i32 = 2;

/// How long a row parked in the hidden "entering" role sits there before
/// animating up into "next". Just needs to span at least one rendered frame,
/// so the animation has a starting point to move *from* instead of popping
/// straight into place — see `promote_and_advance`.
const LYRIC_ENTER_DELAY_MS: u64 = 32;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

type TrackId = (String, String, u64);
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
type TrayHandle = Tray;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
type TrayHandle = ();

/// Identity is (title, artist) only, not duration_ms: some GSMTC sources
/// (notably browser-based players) report a duration that jitters by a few ms
/// poll-to-poll, which would otherwise look like a new track on every poll —
/// clearing lyrics continuously and discarding every fetch response before it
/// ever lands, because by the time it arrives `current_track` has already
/// "changed" again.
fn same_track(a: &TrackId, b: &TrackId) -> bool {
    a.0 == b.0 && a.1 == b.1
}

/// Ordinary one-line advance: the row already showing `line` (last cycle's
/// "next") is promoted into the "current" role — its text never changes, so
/// it can animate smoothly. The retiring row is instantly (invisibly)
/// repurposed to hold the new `next_line`, then, after `LYRIC_ENTER_DELAY_MS`
/// has given it a frame to render in that hidden state, released to animate
/// up into the "next" role — the "next line rises into view" motion.
fn promote_and_advance(
    ui: &MainWindow,
    weak: &slint::Weak<MainWindow>,
    current_slot_is_a: bool,
    next_line: &str,
) {
    if current_slot_is_a {
        ui.set_line_b_role(ROLE_CURRENT);
        ui.set_line_a_instant(true);
        ui.set_line_a_text(next_line.into());
        ui.set_line_a_role(ROLE_ENTERING);
    } else {
        ui.set_line_a_role(ROLE_CURRENT);
        ui.set_line_b_instant(true);
        ui.set_line_b_text(next_line.into());
        ui.set_line_b_role(ROLE_ENTERING);
    }

    // The retiring slot is whichever one just got parked in ROLE_ENTERING.
    let entering_is_a = current_slot_is_a;
    let weak = weak.clone();
    slint::Timer::single_shot(Duration::from_millis(LYRIC_ENTER_DELAY_MS), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if entering_is_a {
            ui.set_line_a_instant(false);
            ui.set_line_a_role(ROLE_NEXT);
        } else {
            ui.set_line_b_instant(false);
            ui.set_line_b_role(ROLE_NEXT);
        }
    });
}

/// A seek, the first line of a track, or any jump that skips lines: there is
/// no row already showing the right text to promote, so both rows snap
/// directly to the correct state instead of sliding through unrelated lines
/// — matching how `SyncEngine` itself snaps the timeline on a seek rather
/// than easing through it.
fn snap_lyric_rows(
    ui: &MainWindow,
    weak: &slint::Weak<MainWindow>,
    current_slot_is_a: bool,
    line: &str,
    next_line: &str,
) {
    ui.set_line_a_instant(true);
    ui.set_line_b_instant(true);
    if current_slot_is_a {
        ui.set_line_a_text(line.into());
        ui.set_line_a_role(ROLE_CURRENT);
        ui.set_line_b_text(next_line.into());
        ui.set_line_b_role(ROLE_NEXT);
    } else {
        ui.set_line_b_text(line.into());
        ui.set_line_b_role(ROLE_CURRENT);
        ui.set_line_a_text(next_line.into());
        ui.set_line_a_role(ROLE_NEXT);
    }

    let weak = weak.clone();
    slint::Timer::single_shot(Duration::from_millis(LYRIC_ENTER_DELAY_MS), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        ui.set_line_a_instant(false);
        ui.set_line_b_instant(false);
    });
}

fn format_mmss(ms: u64) -> String {
    let total_secs = ms / 1_000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes}:{seconds:02}")
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn create_tray() -> Option<TrayHandle> {
    // Same thread requirement as the hotkey manager, and the same
    // must-stay-alive caveat: dropping it removes the icon from the tray.
    // This is the only way to quit, so failing to create it is worth shouting
    // about.
    match Tray::new() {
        Ok(tray) => Some(tray),
        Err(e) => {
            crate::log!("could not create tray icon — no way to quit from the UI: {e}");
            None
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn create_tray() -> Option<TrayHandle> {
    None
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn tray_state(tray: Option<&TrayHandle>) -> (bool, bool) {
    let command = tray.and_then(Tray::poll);
    (
        matches!(command, Some(TrayCommand::Quit)),
        matches!(command, Some(TrayCommand::ToggleVisibility)),
    )
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn tray_state(_tray: Option<&TrayHandle>) -> (bool, bool) {
    (false, false)
}

/// Ctrl-Z (SIGTSTP) merely suspends the process — normal job-control
/// behaviour, not a hang, and `fg`/SIGCONT resumes it fine. But nothing
/// otherwise installs a SIGINT/SIGTERM handler, so Ctrl-C or a bare `kill`
/// hard-kills the process mid-event-loop instead of going through the same
/// quit path as the tray's "Quit Lyra". `quit_event_loop()` is documented as
/// callable from any thread, so this runs straight from the watcher thread
/// rather than bouncing through `invoke_from_event_loop`.
#[cfg(unix)]
fn install_signal_handler() {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = match Signals::new([SIGINT, SIGTERM]) {
        Ok(signals) => signals,
        Err(e) => {
            crate::log!("could not install signal handler: {e}");
            return;
        }
    };
    thread::spawn(move || {
        if let Some(signal) = signals.forever().next() {
            crate::log!("received signal {signal}, quitting");
            slint::quit_event_loop().ok();
        }
    });
}

#[cfg(not(unix))]
fn install_signal_handler() {}

pub fn run() {
    install_signal_handler();

    let (np_tx, np_rx) = mpsc::channel::<NowPlaying>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<MediaCommand>();
    let (lyrics_req_tx, lyrics_req_rx) = mpsc::channel::<TrackId>();
    let (lyrics_res_tx, lyrics_res_rx) = mpsc::channel::<(TrackId, Vec<LyricLine>)>();

    // Transport commands run on this same thread rather than the UI thread:
    // platform readers may rely on thread-affine APIs and some commands can
    // block briefly, so this keeps the card responsive.
    thread::spawn(move || {
        let mut reader = PlatformReader::new();
        loop {
            match reader.poll() {
                Ok(Some(np)) => {
                    if np_tx.send(np).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => crate::log!("media poll error: {e}"),
            }

            // Doubles as the poll interval, but returns the instant a command
            // arrives — so a button press acts immediately instead of waiting
            // out the rest of the tick, and the poll right after it refreshes
            // the card with the result.
            match cmd_rx.recv_timeout(POLL_INTERVAL) {
                Ok(command) => {
                    if let Err(e) = reader.execute(command) {
                        crate::log!("media command {command:?} failed: {e}");
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
                crate::log!("could not open lyrics cache: {e}");
                return;
            }
        };

        for (title, artist, duration_ms) in lyrics_req_rx {
            let lines = match cache.get(&title, &artist, duration_ms) {
                Some(lines) if !lines.is_empty() => {
                    crate::log!(
                        "lyrics: cache hit for {title} - {artist}: {} lines",
                        lines.len()
                    );
                    lines
                }
                _ => match LyricsFetcher::fetch(&title, &artist, duration_ms) {
                    Ok(lines) => {
                        crate::log!(
                            "lyrics: fetched {title} - {artist} ({duration_ms}ms): {} lines",
                            lines.len()
                        );
                        if !lines.is_empty()
                            && let Err(e) = cache.set(&title, &artist, duration_ms, &lines)
                        {
                            crate::log!("could not cache lyrics: {e}");
                        }
                        lines
                    }
                    Err(e) => {
                        crate::log!("lyrics: fetch failed for {title} - {artist}: {e}");
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
            crate::log!("could not create overlay window: {e}");
            return;
        }
    };
    let weak = overlay.as_weak();

    overlay.on_media_command(move |command| {
        if cmd_tx.send(command).is_err() {
            crate::log!("media thread is gone, dropping {command:?}");
        }
    });

    // Must be created on this (the UI event loop's) thread: the underlying
    // platform event source is pumped by the same event loop started below via
    // `overlay.run()`.
    let hotkeys = match HotkeyManager::new() {
        Ok(hotkeys) => Some(hotkeys),
        Err(e) => {
            crate::log!("could not register global hotkey, toggle disabled: {e}");
            None
        }
    };

    let tray = create_tray();

    let mut engine = SyncEngine::new();
    let mut current_track: Option<TrackId> = None;
    let mut last_line: Option<String> = None;
    let mut last_next_line: Option<String> = None;
    // Which Slint slot (line-a vs line-b) is currently playing the "current"
    // role — flips every advance so the row that already shows the right
    // text is the one promoted, instead of always writing to the same slot.
    let mut current_slot_is_a = true;

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(66),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };

            let (quit_requested, tray_toggle_requested) = tray_state(tray.as_ref());
            if quit_requested {
                crate::log!("quit requested from tray");
                // Ends `run_event_loop_until_quit()`, so `run()` returns and
                // the process exits normally.
                slint::quit_event_loop().ok();
                return;
            }

            let toggle_requested = hotkeys.as_ref().is_some_and(HotkeyManager::poll_toggle)
                || tray_toggle_requested;
            if toggle_requested {
                let window = ui.window();
                let toggled = if window.is_visible() {
                    window.hide()
                } else {
                    window.show()
                };
                if let Err(e) = toggled {
                    crate::log!("could not toggle overlay visibility: {e}");
                }
            }

            while let Ok(np) = np_rx.try_recv() {
                let track_id = (np.title.clone(), np.artist.clone(), np.duration_ms);
                let changed = match &current_track {
                    Some(current) => !same_track(current, &track_id),
                    None => true,
                };
                if changed {
                    crate::log!(
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
                        crate::log!("lyrics: applying {} lines to engine", lines.len());
                        engine.set_lyrics(lines);
                    }
                    _ => {
                        crate::log!(
                            "lyrics: discarding stale response for {} - {}",
                            track_id.0, track_id.1
                        );
                    }
                }
            }

            let now = Instant::now();

            let line = engine.tick(now).unwrap_or_default();
            if last_line.as_ref() != Some(&line) {
                let next_line = engine.next_line().to_string();
                // Only true for an ordinary one-line advance, where the row
                // already showing `line` (last cycle's "next") can simply be
                // promoted. Anything else — the first line of a track, or a
                // seek that skips lines — has no such row to promote.
                let advanced_by_one = last_next_line.as_deref() == Some(line.as_str());

                if advanced_by_one {
                    promote_and_advance(&ui, &weak, current_slot_is_a, &next_line);
                    current_slot_is_a = !current_slot_is_a;
                } else {
                    snap_lyric_rows(&ui, &weak, current_slot_is_a, &line, &next_line);
                }

                last_line = Some(line);
                last_next_line = Some(next_line);
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
        crate::log!("overlay window error: {e}");
    }
}

use std::time::Instant;

use crate::domain::NowPlaying;
use crate::lyrics::LyricLine;

/// A fresh poll whose position differs from the interpolated guess by more than
/// this is treated as a seek (snap) rather than ordinary drift (ease).
const SEEK_THRESHOLD_MS: i64 = 1000;
/// Fraction of a drift correction applied per poll when easing toward the
/// reported position.
const DRIFT_EASE_ALPHA: f64 = 0.2;
/// How many consecutive polls that predate an issued seek may be discarded
/// before the engine trusts the source again. Without a bound, a source whose
/// reported timestamp never advances would have every poll after a seek
/// discarded for the rest of the track.
const MAX_STALE_POLLS_AFTER_SEEK: u8 = 3;

pub struct SyncEngine {
    lines: Vec<LyricLine>,
    current_track: Option<(String, String, u64)>,
    last_poll: Option<NowPlaying>,
    baseline_ms: u64,
    upcoming_line: String,
    /// When the last locally-issued seek was applied, and how many polls have
    /// been discarded as predating it. Cleared as soon as a poll catches up.
    seek_at: Option<Instant>,
    stale_polls_after_seek: u8,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_track: None,
            last_poll: None,
            baseline_ms: 0,
            upcoming_line: String::new(),
            seek_at: None,
            stale_polls_after_seek: 0,
        }
    }

    /// Only the caller controls the lyric lines — the engine never clears them
    /// on its own, including on track change. That lifecycle stays owned by
    /// whoever drives this engine (fetch, cache, `set_lyrics`).
    pub fn set_lyrics(&mut self, lines: Vec<LyricLine>) {
        self.lines = lines;
    }

    pub fn on_poll(&mut self, np: NowPlaying) {
        let track_id = (np.title.clone(), np.artist.clone(), np.duration_ms);
        // Identity is (title, artist) only, not duration_ms: some GSMTC sources
        // (notably browser-based players) report a duration that jitters by a
        // few ms poll-to-poll, which would otherwise look like a new track on
        // every single poll and reset the timeline continuously.
        let track_changed = match &self.current_track {
            Some((title, artist, _)) => *title != np.title || *artist != np.artist,
            None => true,
        };

        // A poll whose position was captured before a seek was issued cannot
        // reflect that seek yet. Trusting it snaps the timeline back to where
        // the track was and then forward again one poll later — a visible
        // double jump on every scrub. Take everything except the timeline from
        // it, and keep the position `seek_to` already established.
        if !track_changed && self.is_stale_after_seek(&np) {
            self.stale_polls_after_seek += 1;
            let received_at = self
                .last_poll
                .as_ref()
                .map_or(np.received_at, |last| last.received_at);
            self.last_poll = Some(NowPlaying { received_at, ..np });
            return;
        }
        self.seek_at = None;

        if track_changed {
            self.baseline_ms = np.position_ms;
            self.current_track = Some(track_id);
        } else if np.duration_ms == 0 {
            // Sources with no real position API (e.g. the Firefox/YouTube
            // Music window-title fallback) always report `position_ms: 0`.
            // Treating that as authoritative would snap the timeline back to
            // zero on every poll's seek check, so instead keep whatever the
            // local wall-clock estimate already is and let it keep advancing.
            self.baseline_ms = self.interpolate(np.received_at);
        } else {
            // Evaluated at `np.received_at`, not the caller's wall-clock time:
            // that's the instant `np.position_ms` is actually authoritative
            // for. Using anything later (e.g. when this poll happens to get
            // processed) would compare against a moving target and bias the
            // diff by however much processing latency there was.
            let guess = self.interpolate(np.received_at);
            let diff = np.position_ms as i64 - guess as i64;
            if diff.abs() > SEEK_THRESHOLD_MS {
                self.baseline_ms = np.position_ms;
            } else {
                let eased = guess as f64 + diff as f64 * DRIFT_EASE_ALPHA;
                self.baseline_ms = eased.max(0.0).round() as u64;
            }
        }

        self.last_poll = Some(np);
    }

    /// Rebases the timeline onto `position_ms` as of `now`, for a seek this app
    /// has just issued. Applied locally instead of waiting for the source to
    /// report the new position back, so the card and the lyric line move the
    /// instant the progress bar is clicked rather than up to a poll interval
    /// later. The source still has the final say: if it declined the seek, its
    /// next reported position is far enough from this to register as a jump and
    /// snap the timeline back.
    ///
    /// No-op before the first poll — there is no timeline to rebase, `tick`
    /// returns `None` regardless, and arming the stale-poll guard that early
    /// would only discard the first real poll of the track.
    pub fn seek_to(&mut self, position_ms: u64, now: Instant) {
        let Some(last) = &mut self.last_poll else {
            return;
        };
        let position_ms = if last.duration_ms > 0 {
            position_ms.min(last.duration_ms)
        } else {
            position_ms
        };
        // Interpolation runs from `last.received_at`, which is when the *source*
        // says its position was current — deliberately stale. Moving it to `now`
        // is what makes the new position take effect immediately instead of
        // being extrapolated forward from a moment that has already passed.
        last.received_at = now;
        self.baseline_ms = position_ms;
        self.seek_at = Some(now);
        self.stale_polls_after_seek = 0;
    }

    fn is_stale_after_seek(&self, np: &NowPlaying) -> bool {
        self.stale_polls_after_seek < MAX_STALE_POLLS_AFTER_SEEK
            && self
                .seek_at
                .is_some_and(|seek_at| np.received_at < seek_at)
    }

    pub fn tick(&mut self, now: Instant) -> Option<String> {
        self.last_poll.as_ref()?;
        let pos = self.interpolate(now);
        let active_index = self.lines.iter().rposition(|line| line.time_ms <= pos);
        let next_index = active_index.map_or(0, |i| i + 1);
        self.upcoming_line = self
            .lines
            .get(next_index)
            .map(|line| line.text.clone())
            .unwrap_or_default();
        active_index.map(|i| self.lines[i].text.clone())
    }

    /// The lyric line after the one `tick` last returned — the next line to
    /// come up, or empty before the first line / past the last one. Reflects
    /// whatever `tick` most recently computed, so call it after `tick`.
    pub fn next_line(&self) -> &str {
        &self.upcoming_line
    }

    pub fn position_ms(&self, now: Instant) -> u64 {
        self.interpolate(now)
    }

    pub fn duration_ms(&self) -> u64 {
        self.last_poll.as_ref().map_or(0, |np| np.duration_ms)
    }

    fn interpolate(&self, now: Instant) -> u64 {
        let Some(last) = &self.last_poll else {
            return self.baseline_ms;
        };
        // A `duration_ms` of 0 means the source can't report a real duration
        // (or position) at all, rather than the track genuinely being zero
        // length — clamping against it would pin the position at 0 forever.
        let cap = |position: u64| {
            if last.duration_ms > 0 {
                position.min(last.duration_ms)
            } else {
                position
            }
        };
        if !last.is_playing {
            return cap(self.baseline_ms);
        }
        let elapsed_ms = now.saturating_duration_since(last.received_at).as_millis() as u64;
        cap(self.baseline_ms.saturating_add(elapsed_ms))
    }
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn now_playing(position_ms: u64, is_playing: bool, received_at: Instant) -> NowPlaying {
        NowPlaying {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            duration_ms: 300_000,
            is_playing,
            position_ms,
            received_at,
            can_play_pause: true,
            can_next: true,
            can_previous: true,
            can_seek: true,
        }
    }

    fn sample_lines() -> Vec<LyricLine> {
        vec![
            LyricLine {
                time_ms: 0,
                text: "line 0".to_string(),
            },
            LyricLine {
                time_ms: 5_000,
                text: "line 1".to_string(),
            },
            LyricLine {
                time_ms: 10_000,
                text: "line 2".to_string(),
            },
        ]
    }

    #[test]
    fn tick_advances_active_line_against_simulated_clock() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(0, true, base));

        assert_eq!(engine.tick(base), Some("line 0".to_string()));
        assert_eq!(
            engine.tick(base + Duration::from_millis(6_000)),
            Some("line 1".to_string())
        );
        assert_eq!(
            engine.tick(base + Duration::from_millis(11_000)),
            Some("line 2".to_string())
        );
    }

    #[test]
    fn freezes_while_paused() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(5_500, false, base));

        let t1 = engine.tick(base + Duration::from_millis(2_000));
        let t2 = engine.tick(base + Duration::from_millis(9_000));

        assert_eq!(t1, Some("line 1".to_string()));
        assert_eq!(t1, t2);
    }

    #[test]
    fn snaps_on_a_sharp_jump() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(0, true, base));

        let poll_time = base + Duration::from_millis(1_000);
        // Reported position (10500ms) is far from the interpolated guess
        // (1000ms) at poll_time — a seek, not drift, so it must snap exactly.
        engine.on_poll(now_playing(10_500, true, poll_time));

        assert_eq!(engine.tick(poll_time), Some("line 2".to_string()));
    }

    #[test]
    fn eases_a_small_drift_between_old_and_new_guesses() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.on_poll(now_playing(0, true, base));

        let poll_time = base + Duration::from_millis(1_000);
        // Interpolated guess at poll_time is 1000ms; reported position drifts
        // ahead by 40ms to 1040ms — small enough to ease rather than snap.
        engine.on_poll(now_playing(1_040, true, poll_time));

        let eased = engine.interpolate(poll_time);
        assert!(eased > 1_000 && eased < 1_040);
    }

    #[test]
    fn tick_before_any_poll_returns_none() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());

        assert_eq!(engine.tick(base), None);
    }

    #[test]
    fn next_line_reflects_the_line_after_current() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(0, true, base));

        assert_eq!(engine.tick(base), Some("line 0".to_string()));
        assert_eq!(engine.next_line(), "line 1");

        assert_eq!(
            engine.tick(base + Duration::from_millis(6_000)),
            Some("line 1".to_string())
        );
        assert_eq!(engine.next_line(), "line 2");
    }

    #[test]
    fn next_line_is_empty_past_the_last_line() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(0, true, base));

        engine.tick(base + Duration::from_millis(11_000));
        assert_eq!(engine.next_line(), "");
    }

    #[test]
    fn next_line_is_the_first_line_before_playback_reaches_it() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(vec![LyricLine {
            time_ms: 5_000,
            text: "line 0".to_string(),
        }]);
        engine.on_poll(now_playing(0, true, base));

        assert_eq!(engine.tick(base), None);
        assert_eq!(engine.next_line(), "line 0");
    }

    #[test]
    fn unknown_duration_source_keeps_advancing_instead_of_resetting() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();

        let mut first = now_playing(0, true, base);
        first.duration_ms = 0;
        engine.on_poll(first);

        // The Firefox/YouTube-Music window-title fallback can't read real
        // position, so it reports 0 on every poll. That must not be treated
        // as an authoritative seek back to the start on every poll.
        let poll_time = base + Duration::from_millis(2_000);
        let mut second = now_playing(0, true, poll_time);
        second.duration_ms = 0;
        engine.on_poll(second);

        let interpolated = engine.interpolate(poll_time);
        assert!(
            interpolated >= 1_900,
            "expected position to keep advancing, got {interpolated}"
        );
    }

    #[test]
    fn seek_to_moves_the_timeline_without_waiting_for_a_poll() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(0, true, base));

        let seek_at = base + Duration::from_millis(500);
        engine.seek_to(10_000, seek_at);

        assert_eq!(engine.position_ms(seek_at), 10_000);
        assert_eq!(engine.tick(seek_at), Some("line 2".to_string()));
    }

    #[test]
    fn seek_to_before_any_poll_is_ignored() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.seek_to(10_000, base);

        // Nothing to rebase — and crucially the stale-poll guard must not be
        // armed, or the first real poll of the track would be discarded.
        engine.on_poll(now_playing(4_000, true, base + Duration::from_millis(500)));
        assert_eq!(engine.position_ms(base + Duration::from_millis(500)), 4_000);
    }

    #[test]
    fn ignores_a_poll_captured_before_the_seek() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.on_poll(now_playing(0, true, base));

        let seek_at = base + Duration::from_millis(500);
        engine.seek_to(60_000, seek_at);
        // Captured 300ms *before* the seek was issued, so it still reports the
        // old position — that is not evidence the seek was refused.
        engine.on_poll(now_playing(200, true, base + Duration::from_millis(200)));

        assert_eq!(engine.position_ms(seek_at), 60_000);
    }

    #[test]
    fn accepts_a_poll_captured_after_the_seek() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.on_poll(now_playing(0, true, base));

        let seek_at = base + Duration::from_millis(500);
        engine.seek_to(60_000, seek_at);
        // Captured after the seek, so it reflects what the source actually did
        // with it — authoritative again, and it landed 1s short of the ask.
        let poll_at = base + Duration::from_millis(1_500);
        engine.on_poll(now_playing(59_000, true, poll_at));

        assert_eq!(engine.position_ms(poll_at), 59_000);
    }

    #[test]
    fn stops_ignoring_stale_polls_when_the_source_clock_never_advances() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.on_poll(now_playing(0, true, base));

        let seek_at = base + Duration::from_millis(500);
        let frozen_at = base + Duration::from_millis(200);
        engine.seek_to(60_000, seek_at);
        for _ in 0..=MAX_STALE_POLLS_AFTER_SEEK {
            engine.on_poll(now_playing(1_000, true, frozen_at));
        }

        // A source stuck at a pre-seek timestamp would otherwise be ignored for
        // the rest of the track; past the bound it wins.
        assert_eq!(engine.position_ms(frozen_at), 1_000);
    }

    #[test]
    fn track_change_resets_timeline_but_keeps_lines() {
        let base = Instant::now();
        let mut engine = SyncEngine::new();
        engine.set_lyrics(sample_lines());
        engine.on_poll(now_playing(9_000, true, base));

        let mut next = now_playing(0, true, base + Duration::from_millis(500));
        next.title = "New Song".to_string();
        engine.on_poll(next);

        assert_eq!(
            engine.tick(base + Duration::from_millis(500)),
            Some("line 0".to_string())
        );
        assert_eq!(engine.lines.len(), 3);
    }
}

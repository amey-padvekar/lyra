use std::time::Instant;

use crate::domain::NowPlaying;
use crate::lyrics::LyricLine;

/// A fresh poll whose position differs from the interpolated guess by more than
/// this is treated as a seek (snap) rather than ordinary drift (ease).
const SEEK_THRESHOLD_MS: i64 = 1000;
/// Fraction of a drift correction applied per poll when easing toward the
/// reported position.
const DRIFT_EASE_ALPHA: f64 = 0.2;

pub struct SyncEngine {
    lines: Vec<LyricLine>,
    current_track: Option<(String, String, u64)>,
    last_poll: Option<NowPlaying>,
    baseline_ms: u64,
    upcoming_line: String,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_track: None,
            last_poll: None,
            baseline_ms: 0,
            upcoming_line: String::new(),
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

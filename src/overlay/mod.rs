use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::AppCache;
use crate::media::MediaCommand;

slint::include_modules!();

const GEOMETRY_CACHE_KEY: &str = "window_geometry.json";
/// How often the geometry watcher samples the window.
const GEOMETRY_POLL: Duration = Duration::from_millis(250);
/// Consecutive unchanged samples before a write. Dragging and resizing both
/// emit a continuous stream of geometry changes, so waiting for it to settle
/// turns a whole gesture into one write instead of one per frame.
const SETTLE_SAMPLES: u32 = 2;

pub struct OverlayWindow {
    window: MainWindow,
    /// Held purely to keep it alive — a dropped `Timer` stops firing.
    _geometry_watcher: slint::Timer,
}

impl OverlayWindow {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = MainWindow::new()?;

        let restored = load_geometry();
        if let Some(geometry) = restored {
            window
                .window()
                .set_position(slint::LogicalPosition::new(geometry.x, geometry.y));
            window
                .window()
                .set_size(slint::LogicalSize::new(geometry.width, geometry.height));
        }

        let weak = window.as_weak();
        window.on_drag_window(move |dx, dy| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            // `dx`/`dy` arrive as Slint `length`, i.e. already logical, so
            // staying in logical space avoids a round-trip through physical.
            let current = logical_position(&window);
            window
                .window()
                .set_position(slint::LogicalPosition::new(current.x + dx, current.y + dy));
        });

        // Position and size are persisted by this watcher rather than from the
        // drag handler, so resizing is covered too — the drag handler only ever
        // saw moves.
        let watcher = slint::Timer::default();
        let weak = window.as_weak();
        let tracker = Rc::new(RefCell::new(GeometryTracker {
            last_seen: None,
            stable_samples: 0,
            saved: restored,
        }));
        watcher.start(slint::TimerMode::Repeated, GEOMETRY_POLL, move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            tracker.borrow_mut().sample(&window);
        });

        Ok(Self {
            window,
            _geometry_watcher: watcher,
        })
    }

    pub fn as_weak(&self) -> slint::Weak<MainWindow> {
        self.window.as_weak()
    }

    /// Routes all three transport buttons through one handler. `Rc` rather
    /// than cloning the closure: Slint callbacks each need their own owner,
    /// and these all run on the UI thread so no synchronisation is needed.
    pub fn on_media_command(&self, handler: impl Fn(MediaCommand) + 'static) {
        let handler = Rc::new(handler);

        let toggle = handler.clone();
        self.window
            .on_toggle_play_pause(move || toggle(MediaCommand::TogglePlayPause));

        let next = handler.clone();
        self.window.on_skip_next(move || next(MediaCommand::Next));

        let previous = handler.clone();
        self.window
            .on_skip_previous(move || previous(MediaCommand::Previous));
    }

    /// Not `self.window.run()` — that convenience method is `show()` +
    /// `run_event_loop()` + `hide()`, and `run_event_loop()` quits the whole
    /// event loop the moment the last window becomes invisible. Since the
    /// hotkey toggle hides this (our only) window, that would silently end
    /// the loop — and the process — on the first toggle-off.
    /// `run_event_loop_until_quit()` keeps running regardless of visibility,
    /// until something calls `slint::quit_event_loop()` (which nothing here
    /// does yet, so the window can be hidden and shown indefinitely).
    pub fn run(&self) -> Result<(), slint::PlatformError> {
        self.window.show()?;
        slint::run_event_loop_until_quit()
    }
}

/// Stored in *logical* pixels, not physical. Slint reports geometry in
/// physical pixels but `set_size` reinterprets what it's given, so a
/// physical-in/physical-out round-trip multiplies by the scale factor on every
/// restart — at 125% scaling the window grew 396x260 -> 495x325 -> ... each
/// launch. Logical units are also the right thing to persist anyway: they keep
/// the card the same apparent size if it moves to a monitor with different DPI.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Geometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct GeometryTracker {
    last_seen: Option<Geometry>,
    stable_samples: u32,
    saved: Option<Geometry>,
}

impl GeometryTracker {
    fn sample(&mut self, window: &MainWindow) {
        // Skip while hidden: the hotkey toggle can leave a hidden window
        // reporting a stale or zeroed rect, which isn't worth persisting.
        if !window.window().is_visible() {
            return;
        }

        let scale = window.window().scale_factor();
        let position = logical_position(window);
        let size = slint::LogicalSize::from_physical(window.window().size(), scale);
        // Rounded so equality is stable: the physical->logical divide yields
        // fractional values that would otherwise jitter between samples and
        // never settle.
        let current = Geometry {
            x: position.x.round(),
            y: position.y.round(),
            width: size.width.round(),
            height: size.height.round(),
        };

        if self.last_seen != Some(current) {
            self.last_seen = Some(current);
            self.stable_samples = 0;
            return;
        }
        if self.stable_samples >= SETTLE_SAMPLES {
            return;
        }

        self.stable_samples += 1;
        if self.stable_samples == SETTLE_SAMPLES && self.saved != Some(current) {
            self.saved = Some(current);
            save_geometry(&current);
        }
    }
}

fn logical_position(window: &MainWindow) -> slint::LogicalPosition {
    let scale = window.window().scale_factor();
    slint::LogicalPosition::from_physical(window.window().position(), scale)
}

fn load_geometry() -> Option<Geometry> {
    let cache = AppCache::new().ok()?;
    let raw = std::fs::read_to_string(cache.path_for(GEOMETRY_CACHE_KEY)).ok()?;
    let geometry: Geometry = serde_json::from_str(&raw).ok()?;
    // A zero/negative or non-finite dimension would leave the window
    // unrecoverable by dragging alone.
    let sane = geometry.width.is_finite()
        && geometry.height.is_finite()
        && geometry.x.is_finite()
        && geometry.y.is_finite()
        && geometry.width > 0.0
        && geometry.height > 0.0;
    sane.then_some(geometry)
}

fn save_geometry(geometry: &Geometry) {
    let Ok(cache) = AppCache::new() else {
        return;
    };
    let Ok(raw) = serde_json::to_string(geometry) else {
        return;
    };
    let _ = std::fs::write(cache.path_for(GEOMETRY_CACHE_KEY), raw);
}

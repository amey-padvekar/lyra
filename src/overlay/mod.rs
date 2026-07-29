use serde::{Deserialize, Serialize};

use crate::cache::AppCache;

slint::include_modules!();

const POSITION_CACHE_KEY: &str = "window_position.json";

pub struct OverlayWindow {
    window: MainWindow,
}

impl OverlayWindow {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = MainWindow::new()?;

        if let Some(pos) = load_position() {
            window
                .window()
                .set_position(slint::PhysicalPosition::new(pos.x, pos.y));
        }

        let weak = window.as_weak();
        window.on_drag_window(move |dx, dy| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let scale = window.window().scale_factor();
            let pos = window.window().position();
            let new_pos = slint::PhysicalPosition::new(
                pos.x + (dx * scale) as i32,
                pos.y + (dy * scale) as i32,
            );
            window.window().set_position(new_pos);
            save_position(new_pos.x, new_pos.y);
        });

        Ok(Self { window })
    }

    pub fn as_weak(&self) -> slint::Weak<MainWindow> {
        self.window.as_weak()
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

#[derive(Serialize, Deserialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

fn load_position() -> Option<SavedPosition> {
    let cache = AppCache::new().ok()?;
    let raw = std::fs::read_to_string(cache.path_for(POSITION_CACHE_KEY)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_position(x: i32, y: i32) {
    let Ok(cache) = AppCache::new() else {
        return;
    };
    let Ok(raw) = serde_json::to_string(&SavedPosition { x, y }) else {
        return;
    };
    let _ = std::fs::write(cache.path_for(POSITION_CACHE_KEY), raw);
}

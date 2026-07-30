//! System tray icon — the app's only way to quit.
//!
//! The overlay is frameless and always-on-top, so it has no close button, and
//! the event loop deliberately outlives a hidden window (see
//! `OverlayWindow::run`). Without this, the only way out is Task Manager.

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const SHOW_HIDE_ID: &str = "lyra.show-hide";
const STARTUP_ID: &str = "lyra.startup";
const QUIT_ID: &str = "lyra.quit";

#[derive(Clone, Copy, Debug)]
pub enum TrayCommand {
    ToggleVisibility,
    ToggleRunOnStartup,
    Quit,
}

pub struct Tray {
    /// Dropping the `TrayIcon` removes it from the notification area, so it
    /// has to outlive the event loop even though nothing reads it again.
    _icon: TrayIcon,
    startup_item: CheckMenuItem,
}

impl Tray {
    /// Must be called on the thread that runs the UI event loop: the tray icon
    /// is backed by a window that only receives its messages while that
    /// thread's message loop is pumped.
    pub fn new(startup_enabled: bool) -> anyhow::Result<Self> {
        let menu = Menu::new();
        let startup_item = CheckMenuItem::with_id(
            STARTUP_ID,
            "Run on startup",
            true,
            startup_enabled,
            None,
        );
        menu.append_items(&[
            &MenuItem::with_id(SHOW_HIDE_ID, "Show / hide", true, None),
            &startup_item,
            &MenuItem::with_id(QUIT_ID, "Quit Lyra", true, None),
        ])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(app_icon()?)
            .with_tooltip("Lyra")
            .build()?;

        Ok(Self {
            _icon: icon,
            startup_item,
        })
    }

    /// Non-blocking; returns the last command seen this tick.
    pub fn poll(&self) -> Option<TrayCommand> {
        let mut command = None;
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == QUIT_ID {
                command = Some(TrayCommand::Quit);
            } else if event.id == SHOW_HIDE_ID {
                command = Some(TrayCommand::ToggleVisibility);
            } else if event.id == STARTUP_ID {
                command = Some(TrayCommand::ToggleRunOnStartup);
            }
        }
        command
    }

    pub fn set_startup_checked(&self, enabled: bool) {
        self.startup_item.set_checked(enabled);
    }
}

/// Drawn in code rather than shipped as an asset — a placeholder green disc
/// matching the progress bar, so the beta isn't stuck with a blank tray slot.
/// Replace with a real `.ico` before a wider release.
fn app_icon() -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 32;
    let centre = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 1.0;

    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            // Fade across the outermost pixel so the edge isn't visibly jagged.
            let alpha = ((radius - distance).clamp(0.0, 1.0) * 255.0) as u8;
            rgba.extend_from_slice(&[0x1e, 0xd7, 0x60, alpha]);
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE)
}

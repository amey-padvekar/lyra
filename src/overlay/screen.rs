//! Keeps the overlay reachable when the display layout changes.
//!
//! Window position is persisted across runs, so a card last left on a second
//! monitor restores to coordinates that no longer exist once that monitor is
//! unplugged. Combined with a frameless window there is no titlebar to grab,
//! which would leave the card invisible and unrecoverable.

use crate::overlay::MainWindow;

#[cfg(target_os = "windows")]
pub fn ensure_on_screen(window: &MainWindow) {
    use slint::ComponentHandle;
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL,
        MONITOR_DEFAULTTOPRIMARY,
    };

    // Slint reports both of these in physical screen coordinates, which is
    // exactly the space the monitor APIs work in — no scale-factor conversion
    // needed, and none of the logical/physical confusion that bit the geometry
    // round-trip.
    let position = window.window().position();
    let size = window.window().size();
    let rect = RECT {
        left: position.x,
        top: position.y,
        right: position.x + size.width as i32,
        bottom: position.y + size.height as i32,
    };

    // MONITOR_DEFAULTTONULL: null means the rect overlaps no display at all.
    let visible = unsafe { !MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_invalid() };
    if visible {
        return;
    }

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    if unsafe { !GetMonitorInfoW(primary, &mut info).as_bool() } {
        crate::log!("could not query primary monitor; leaving window where it is");
        return;
    }

    // Bottom-right of the primary work area, matching the card's intended
    // resting place. `rcWork` excludes the taskbar.
    const MARGIN: i32 = 24;
    let x = (info.rcWork.right - size.width as i32 - MARGIN).max(info.rcWork.left);
    let y = (info.rcWork.bottom - size.height as i32 - MARGIN).max(info.rcWork.top);

    crate::log!(
        "saved position {},{} is off-screen; moving to {x},{y}",
        position.x,
        position.y
    );
    window
        .window()
        .set_position(slint::PhysicalPosition::new(x, y));
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_on_screen(_window: &MainWindow) {}

pub mod app;
pub mod cache;
pub mod domain;
pub mod hotkey;
pub mod logging;
pub mod lyrics;
pub mod media;
pub mod overlay;
pub mod sync;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub mod tray;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub trait MediaReader: Send {
    fn poll(&mut self) -> anyhow::Result<Option<crate::domain::NowPlaying>>;
}

#[cfg(target_os = "windows")]
pub use windows::GsmtcReader as PlatformReader;

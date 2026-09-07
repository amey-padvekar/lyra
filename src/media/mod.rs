#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
mod mediaremote;
#[cfg(target_os = "windows")]
pub mod windows;

/// A transport command sent *to* the active media session.
#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    TogglePlayPause,
    Next,
    Previous,
    /// Jump to an absolute position in the current track. Only sent when the
    /// source reports `NowPlaying::can_seek`.
    Seek { position_ms: u64 },
}

pub trait MediaReader: Send {
    fn poll(&mut self) -> anyhow::Result<Option<crate::domain::NowPlaying>>;

    /// Best-effort: a source may legitimately refuse a command, which is not
    /// an error here. `Err` is reserved for actually failing to talk to the
    /// platform's media API.
    fn execute(&mut self, command: MediaCommand) -> anyhow::Result<()>;
}

#[cfg(target_os = "macos")]
pub use macos::HybridReader as PlatformReader;
#[cfg(target_os = "windows")]
pub use windows::GsmtcReader as PlatformReader;

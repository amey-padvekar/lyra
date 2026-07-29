use std::thread;
use std::time::{Duration, Instant};

use anyhow::bail;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows_future::{AsyncStatus, IAsyncOperation};

use crate::domain::NowPlaying;
use crate::media::MediaReader;

/// Zero-field marker: the GSMTC COM handles it touches are `!Send` (no explicit
/// `Send`/`Sync` impl for the generated interface wrapper types), so nothing from
/// a `poll()` call may be stored on `self` across calls — every handle is
/// requested fresh and dropped before returning.
pub struct GsmtcReader;

impl MediaReader for GsmtcReader {
    fn poll(&mut self) -> anyhow::Result<Option<NowPlaying>> {
        let manager = wait(SessionManager::RequestAsync()?)?;

        let session = match manager.GetCurrentSession() {
            Ok(session) => session,
            Err(_) => return Ok(None),
        };

        let props = wait(session.TryGetMediaPropertiesAsync()?)?;
        let timeline = session.GetTimelineProperties()?;
        let playback_info = session.GetPlaybackInfo()?;

        let position_ms = (timeline.Position()?.Duration / 10_000).max(0) as u64;
        let duration_ms = (timeline.EndTime()?.Duration / 10_000).max(0) as u64;
        let is_playing = playback_info.PlaybackStatus()? == PlaybackStatus::Playing;
        let received_at = filetime_to_instant(timeline.LastUpdatedTime()?.UniversalTime);

        Ok(Some(NowPlaying {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            duration_ms,
            is_playing,
            position_ms,
            received_at,
        }))
    }
}

/// Converts GSMTC's `LastUpdatedTime` — a Windows FILETIME (100ns ticks since
/// 1601-01-01 UTC) — into a monotonic `Instant`, by anchoring against a
/// same-moment `(Instant, SystemTime)` pair.
///
/// `Position()` is only accurate as of `LastUpdatedTime`, not as of whenever we
/// happen to poll it: many sources (notably browsers) don't push a fresh
/// position every frame. Stamping `NowPlaying::received_at` with our own poll
/// time instead of this value made `SyncEngine`'s interpolation extrapolate
/// from the wrong reference point, drifting out of sync with real playback by
/// however stale the source's last update was.
fn filetime_to_instant(universal_time_100ns: i64) -> Instant {
    const FILETIME_TO_UNIX_100NS: i64 = 116_444_736_000_000_000;
    let unix_100ns = (universal_time_100ns - FILETIME_TO_UNIX_100NS).max(0);
    let last_updated =
        std::time::SystemTime::UNIX_EPOCH + Duration::from_nanos(unix_100ns as u64 * 100);

    let system_now = std::time::SystemTime::now();
    let instant_now = Instant::now();

    match system_now.duration_since(last_updated) {
        Ok(age) => instant_now.checked_sub(age).unwrap_or(instant_now),
        Err(_) => instant_now,
    }
}

/// Busy-waits a WinRT `IAsyncOperation` to completion. Avoids the callback-based
/// `AsyncOperationCompletedHandler` machinery, at the cost of blocking the calling
/// thread with a poll loop — acceptable since `MediaReader::poll()` already runs
/// on its own dedicated worker thread.
fn wait<T: windows::core::RuntimeType + 'static>(op: IAsyncOperation<T>) -> anyhow::Result<T> {
    loop {
        match op.Status()? {
            AsyncStatus::Completed => return Ok(op.GetResults()?),
            AsyncStatus::Error | AsyncStatus::Canceled => {
                bail!("GSMTC async operation did not complete: {:?}", op.Status()?);
            }
            _ => thread::sleep(Duration::from_millis(10)),
        }
    }
}

//! Diagnostics for release builds, where there is no console to print to.
//!
//! Release binaries set `windows_subsystem = "windows"` so no console window
//! appears alongside the overlay, which also means `stderr` goes nowhere. This
//! writes the same messages to a file in the cache directory instead, so a beta
//! tester reporting "lyrics didn't load" has something to attach.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::cache::AppCache;

const LOG_KEY: &str = "lyra.log";
const PREVIOUS_LOG_KEY: &str = "lyra.previous.log";
/// Past this, the current log is rotated to `lyra.previous.log`. Two files
/// bound disk use while still keeping the run before the one that broke.
const MAX_LOG_BYTES: u64 = 1_000_000;

static LOG_FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
static STARTED_AT: OnceLock<Instant> = OnceLock::new();

/// Safe to skip — logging then falls back to stderr only.
pub fn init() {
    STARTED_AT.get_or_init(Instant::now);
    LOG_FILE.get_or_init(open_log);

    let epoch_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    write(format_args!(
        "lyra {} starting (unix time {epoch_secs}; timestamps below are elapsed since this line)",
        env!("CARGO_PKG_VERSION")
    ));
}

fn open_log() -> Option<Mutex<File>> {
    let cache = AppCache::new().ok()?;
    let path = cache.path_for(LOG_KEY);

    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, cache.path_for(PREVIOUS_LOG_KEY));
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
        .map(Mutex::new)
}

pub fn write(args: fmt::Arguments) {
    let elapsed = STARTED_AT.get_or_init(Instant::now).elapsed();
    let stamp = format!(
        "{:02}:{:02}:{:02}.{:03}",
        elapsed.as_secs() / 3600,
        (elapsed.as_secs() % 3600) / 60,
        elapsed.as_secs() % 60,
        elapsed.subsec_millis()
    );

    // Still emit to stderr: harmless when there is no console, and keeps
    // `cargo run` during development behaving as it always did.
    eprintln!("[{stamp}] {args}");

    if let Some(Some(file)) = LOG_FILE.get() {
        // A poisoned mutex only means some other thread panicked mid-write;
        // the file handle is still usable, so log anyway rather than lose it.
        let mut file = match file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };
        let _ = writeln!(file, "[{stamp}] {args}");
        let _ = file.flush();
    }
}

/// Same shape as `eprintln!`, but lands in the log file too.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::logging::write(format_args!($($arg)*))
    };
}

// Throwaway probe verifying transport commands reach the live GSMTC session.
// Run with: cargo run --example media_control_probe
//
// Side effect: this really does control playback. It pauses and un-pauses
// (net zero), skips forward and back, then seeks to a third of the way in.
//
// Note the 2.5s settle after each command. Sources can take a second or two to
// republish metadata after a skip — sampling sooner shows the *old* track and
// reads as "the command was ignored" when it actually landed.

use std::thread;
use std::time::Duration;

use lyra::media::{MediaCommand, MediaReader, PlatformReader};

fn show(reader: &mut PlatformReader, label: &str) {
    match reader.poll() {
        Ok(Some(np)) => println!(
            "{label}: {} - {} @ {}ms/{}ms playing={} | caps: play/pause={} next={} prev={} seek={}",
            np.artist,
            np.title,
            np.position_ms,
            np.duration_ms,
            np.is_playing,
            np.can_play_pause,
            np.can_next,
            np.can_previous,
            np.can_seek
        ),
        Ok(None) => println!("{label}: (nothing playing)"),
        Err(e) => println!("{label}: poll error: {e}"),
    }
}

fn send(reader: &mut PlatformReader, command: MediaCommand) {
    match reader.execute(command) {
        Ok(()) => println!("  -> sent {command:?}"),
        Err(e) => println!("  -> {command:?} FAILED: {e}"),
    }
    thread::sleep(Duration::from_millis(2500));
}

fn main() {
    let mut reader = PlatformReader::new();

    show(&mut reader, "initial");

    send(&mut reader, MediaCommand::TogglePlayPause);
    show(&mut reader, "after pause");

    send(&mut reader, MediaCommand::TogglePlayPause);
    show(&mut reader, "after resume");

    send(&mut reader, MediaCommand::Next);
    show(&mut reader, "after next");

    send(&mut reader, MediaCommand::Previous);
    show(&mut reader, "after previous");

    // The one command whose effect is measurable from the probe itself: the
    // position printed afterwards should be near the requested target, which is
    // what says the source accepted the position rather than ignoring it.
    let target_ms = match reader.poll() {
        Ok(Some(np)) if np.duration_ms > 0 => np.duration_ms / 3,
        _ => {
            println!("no duration reported; skipping the seek probe");
            return;
        }
    };
    send(
        &mut reader,
        MediaCommand::Seek {
            position_ms: target_ms,
        },
    );
    show(&mut reader, &format!("after seek to {target_ms}ms"));
}

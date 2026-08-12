// Throwaway probe verifying transport commands reach the live GSMTC session.
// Run with: cargo run --example media_control_probe
//
// Side effect: this really does control playback. It pauses and un-pauses
// (net zero), then skips forward and back.
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
            "{label}: {} - {} @ {}ms playing={} | caps: play/pause={} next={} prev={}",
            np.artist,
            np.title,
            np.position_ms,
            np.is_playing,
            np.can_play_pause,
            np.can_next,
            np.can_previous
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
}

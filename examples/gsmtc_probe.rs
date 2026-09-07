// Throwaway probe for Step 2's gate (see IMPLEMENTATION_PLAN.md).
// Run with: cargo run --example gsmtc_probe
// Polls GsmtcReader once a second and prints {title, artist, position}.

#[cfg(target_os = "windows")]
fn main() {
    use lyra::media::windows::GsmtcReader;
    use lyra::media::MediaReader;
    use std::time::Duration;

    let mut reader = GsmtcReader;
    for _ in 0..10 {
        match reader.poll() {
            Ok(Some(np)) => println!(
                "{} - {} [{}ms / {}ms] playing={} seekable={}",
                np.artist, np.title, np.position_ms, np.duration_ms, np.is_playing, np.can_seek
            ),
            Ok(None) => println!("(nothing playing)"),
            Err(e) => println!("poll error: {e:?}"),
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("gsmtc_probe is Windows-only");
}

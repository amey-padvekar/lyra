// Throwaway probe: does `slint::quit_event_loop()` actually terminate
// `run_event_loop_until_quit()` and let the process exit cleanly?
//
// This is the mechanism the tray's Quit item relies on, and it's the one part
// of that path that can't be checked by clicking nothing. Run with:
//   cargo run --example quit_mechanism_probe
// Expected: prints "quitting", then "event loop returned", then exits 0.

use std::time::Duration;

use lyra::overlay::MainWindow;
use slint::ComponentHandle;

fn main() {
    let window = MainWindow::new().expect("window");
    window.show().expect("show");

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(500),
        move || {
            println!("quitting");
            slint::quit_event_loop().ok();
        },
    );

    slint::run_event_loop_until_quit().expect("event loop");
    println!("event loop returned — process will now exit normally");
}

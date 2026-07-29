// No console window in release builds — this is a GUI overlay, and a stray
// black console beside it looks broken. Debug builds keep the console so
// `cargo run` still prints. Either way diagnostics also go to the log file
// (see `logging`), which is what release builds rely on.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lyra::logging::init();
    lyra::app::run();
}

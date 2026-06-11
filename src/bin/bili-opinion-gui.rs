// Hide the extra Windows console window for the GUI binary only.
// The CLI binary still uses src/main.rs and keeps its console.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    bili_opinion::gui::gpui_app::run();
}

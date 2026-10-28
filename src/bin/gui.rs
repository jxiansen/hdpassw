//! `hdpassw-gui` — thin launcher for the desktop GUI.
//!
//! All the interface code lives in the library (`hdpassw::gui`) so it can be
//! rendered headlessly by integration tests; this binary only sets the
//! Windows subsystem flag and calls the entry point.

// Release builds on Windows are GUI apps: don't pop a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    hdpassw::gui::run()
}

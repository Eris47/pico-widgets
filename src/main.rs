// Don't open a black console window behind the app.
#![windows_subsystem = "windows"]

mod config;
mod draw;

#[cfg(windows)]
mod app;

#[cfg(windows)]
mod settings;

#[cfg(windows)]
mod spotify;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("pico-widgets only runs on Windows.");
}

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod cert;
mod config;
mod credentials;
mod download;
#[cfg(windows)]
mod gui;
mod paths;
mod project;
#[cfg(windows)]
mod proxy;
mod python;
mod report;
mod runner;
mod util;

#[cfg(windows)]
fn main() {
    if let Err(e) = gui::run() {
        unsafe {
            use windows::core::PCWSTR;
            use windows::Win32::UI::WindowsAndMessaging::{
                MessageBoxW, MB_ICONERROR, MB_OK,
            };
            let msg: Vec<u16> = format!("{e:#}\0").encode_utf16().collect();
            let title: Vec<u16> = "hamid-mahdavi-client\0".encode_utf16().collect();
            MessageBoxW(
                None,
                PCWSTR(msg.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hamid-mahdavi-client is Windows-only. Build on Windows with `cargo build --release`.");
    std::process::exit(1);
}

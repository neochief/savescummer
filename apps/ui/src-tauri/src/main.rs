#![cfg_attr(windows, windows_subsystem = "windows")]

mod bridge;

fn main() {
    bridge::run();
}

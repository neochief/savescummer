//! The fake game: copied to different paths to act as different games, run
//! several times to act as one game with several processes. It can write
//! saves, hold a save open, launch a child and exit, show a window, crash,
//! and quits when a quit file appears.
//!
//! Arguments (all optional, repeatable where it makes sense):
//!
//! - `--write <path>=<text>`: write a save file at start.
//! - `--hold <path>`: keep a file open without delete sharing until exit.
//! - `--launch <exe> [args…] --`: start another program, then keep going.
//! - `--exit-now`: exit right after starting (a launcher).
//! - `--crash-after <ms>`: exit abnormally after a while.
//! - `--run-ms <ms>`: exit normally after a while.
//! - `--quit-file <path>`: exit when this file appears.
//! - `--window`: show a window (Windows only).

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut held = Vec::new();
    let mut quit_file: Option<PathBuf> = None;
    let mut run_ms: Option<u64> = None;
    let mut crash_ms: Option<u64> = None;
    let mut exit_now = false;
    let mut window = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--write" => {
                let (path, text) = args[i + 1].split_once('=').expect("--write <path>=<text>");
                if let Some(parent) = std::path::Path::new(path).parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, text).unwrap();
                i += 2;
            }
            "--hold" => {
                held.push(hold(&args[i + 1]));
                i += 2;
            }
            "--launch" => {
                let end = args[i + 1..].iter().position(|a| a == "--").map(|p| p + i + 1).unwrap_or(args.len());
                let exe = &args[i + 1];
                let rest = &args[i + 2..end];
                // A launcher leaves its game running on purpose.
                #[allow(clippy::zombie_processes)]
                std::process::Command::new(exe).args(rest).spawn().expect("launch");
                i = end + 1;
            }
            "--exit-now" => {
                exit_now = true;
                i += 1;
            }
            "--run-ms" => {
                run_ms = args[i + 1].parse().ok();
                i += 2;
            }
            "--crash-after" => {
                crash_ms = args[i + 1].parse().ok();
                i += 2;
            }
            "--quit-file" => {
                quit_file = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--window" => {
                window = true;
                i += 1;
            }
            other => panic!("unknown argument {other}"),
        }
    }
    if exit_now {
        return;
    }
    if window {
        show_window();
    }
    let start = Instant::now();
    loop {
        if quit_file.as_ref().is_some_and(|q| q.exists()) {
            break;
        }
        if run_ms.is_some_and(|ms| start.elapsed() >= Duration::from_millis(ms)) {
            break;
        }
        if crash_ms.is_some_and(|ms| start.elapsed() >= Duration::from_millis(ms)) {
            std::process::exit(101);
        }
        pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(held);
}

#[cfg(windows)]
fn hold(path: &str) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    // Read sharing only: renaming or deleting it fails while we run, like a
    // game that keeps its save open.
    std::fs::OpenOptions::new().read(true).share_mode(1).open(path).expect("hold")
}

#[cfg(not(windows))]
fn hold(path: &str) -> std::fs::File {
    std::fs::File::open(path).expect("hold")
}

#[cfg(windows)]
fn show_window() {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let class: Vec<u16> = "SaveScummerFakeGame\0".encode_utf16().collect();
    // SAFETY: a plain top-level window with the default window procedure.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            320,
            200,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        SetForegroundWindow(hwnd);
    }
}

#[cfg(not(windows))]
fn show_window() {}

#[cfg(windows)]
fn pump() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    // SAFETY: draining this thread's message queue.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump() {}

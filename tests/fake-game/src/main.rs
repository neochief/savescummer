use clap::Parser;
#[derive(Parser)]
struct Options {
    #[arg(long, default_value_t = 60000)]
    duration_ms: u64,
    #[arg(long, default_value_t = 0)]
    window_delay_ms: u64,
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    ready_file: Option<std::path::PathBuf>,
}
fn main() {
    let options = Options::parse();
    if let Some(path) = &options.ready_file {
        std::fs::write(path, std::process::id().to_string()).unwrap();
    }
    #[cfg(windows)]
    if !options.headless {
        window(&options);
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(options.duration_ms));
}
#[cfg(windows)]
fn window(options: &Options) {
    use windows_sys::Win32::{
        Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
    };
    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        if message == WM_DESTROY {
            unsafe {
                PostQuitMessage(0);
            }
            0
        } else {
            unsafe { DefWindowProcW(window, message, w, l) }
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(options.window_delay_ms));
    let class = "SaveScummerFakeGame\0".encode_utf16().collect::<Vec<_>>();
    let title = "SaveScummer test game\0".encode_utf16().collect::<Vec<_>>();
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let descriptor = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        assert_ne!(RegisterClassW(&descriptor), 0);
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            320,
            200,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        assert!(!window.is_null());
        let start = std::time::Instant::now();
        let mut message: MSG = std::mem::zeroed();
        while start.elapsed().as_millis() < options.duration_ms as u128 {
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if message.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        DestroyWindow(window);
    }
}

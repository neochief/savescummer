//! Optional OS presentation adapters. No game selection or save policy lives here.
use std::{io, path::Path, sync::mpsc};

/// The host is a windowless Windows executable. Explicit terminal launches may
/// attach to the caller's console; autostart never allocates a console window.
pub fn attach_parent_console(enabled: bool) {
    #[cfg(windows)]
    if enabled {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = enabled;
}

#[derive(Debug, Clone, Copy)]
pub enum DesktopEvent {
    Save,
    Load,
    Open,
    Exit,
}

pub trait StartupRegistration: Send + Sync {
    fn set_enabled(&self, enabled: bool) -> io::Result<()>;
}

pub struct NativeStartup {
    pub host: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
    pub desktop: Option<std::path::PathBuf>,
}
impl StartupRegistration for NativeStartup {
    fn set_enabled(&self, enabled: bool) -> io::Result<()> {
        #[cfg(windows)]
        {
            super::windows::set_startup(
                &self.host,
                &self.data_dir,
                self.desktop.as_deref(),
                enabled,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = enabled;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "startup registration is Windows-only",
            ))
        }
    }
}

pub fn explore(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        super::windows::explore(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native Explore is Windows-only",
        ))
    }
}

pub enum DesktopCommand {
    Failure(String),
    Stop,
}
/// Cloneable nonblocking notification sink; does not extend operation locks.
#[derive(Clone, Default)]
pub struct Notifications(pub Option<mpsc::Sender<DesktopCommand>>);
impl Notifications {
    pub fn failure(&self, message: impl Into<String>) {
        if let Some(sender) = &self.0 {
            let _ = sender.send(DesktopCommand::Failure(message.into()));
        }
    }
}
pub struct Desktop {
    pub notifications: Notifications,
    pub warnings: Vec<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Desktop {
    pub fn start(events: mpsc::Sender<DesktopEvent>) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let (tx, rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let thread =
                std::thread::spawn(move || super::windows_desktop::run(events, rx, ready_tx));
            match ready_rx.recv().map_err(io::Error::other)? {
                Ok(warnings) => Ok(Self {
                    notifications: Notifications(Some(tx)),
                    warnings,
                    thread: Some(thread),
                }),
                Err(error) => {
                    let _ = thread.join();
                    Err(error)
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = events;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "desktop integration is Windows-only",
            ))
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        if let Some(tx) = &self.notifications.0 {
            let _ = tx.send(DesktopCommand::Stop);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

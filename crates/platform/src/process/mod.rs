//! Process helpers the host and CLI share: starting a program that outlives
//! its starter, and ending this process the way a crash would.

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "unix.rs")]
mod imp;

pub use imp::{detach, hard_exit};

/// Starts a game without keeping the host attached to its lifetime.
pub fn launch_game(exe: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let bundle = exe.ancestors().find(|path| path.extension().is_some_and(|ext| ext == "app"));
    #[cfg(target_os = "macos")]
    let mut command = if let Some(bundle) = bundle {
        let mut command = std::process::Command::new("/usr/bin/open");
        command.arg("-a").arg(bundle);
        command
    } else {
        std::process::Command::new(exe)
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = std::process::Command::new(exe);

    if exe.extension().is_none_or(|ext| ext != "app")
        && let Some(parent) = exe.parent()
    {
        command.current_dir(parent);
    }
    detach(&mut command);
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Steam owns launching its installs, including Cloud sync and Proton.
pub fn launch_steam(app_id: u64) -> std::io::Result<()> {
    let url = format!("steam://rungameid/{app_id}");
    #[cfg(windows)]
    return crate::win::shell_open(std::path::Path::new(&url));
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "/usr/bin/open" } else { "xdg-open" };
        let status = std::process::Command::new(opener).arg(url).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("Steam URL opener exited with {status}")))
        }
    }
}

/// Sends a normal close request to one observed game process. Never force-kills it.
pub fn request_game_close(pid: u32) -> std::io::Result<()> {
    imp::request_game_close(pid)
}

/// Launch a packaged macOS host as an app, so macOS attributes privacy
/// requests to SaveScummer even when a CLI or IDE initiated the launch.
/// Bare executables (including test fixtures) have no app bundle to open.
pub fn bundled_host_command(exe: &std::path::Path) -> Option<std::process::Command> {
    #[cfg(target_os = "macos")]
    {
        let bundle = exe.ancestors().nth(3).filter(|path| path.extension().is_some_and(|ext| ext == "app"))?;
        if exe.file_name()? != "SaveScummer" || !bundle.join("Contents/MacOS/SaveScummer").is_file() {
            return None;
        }
        let mut command = std::process::Command::new("/usr/bin/open");
        command.args(["-n", "-g", "-j", "-a"]).arg(bundle).arg("--args");
        Some(command)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = exe;
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn packaged_host_uses_launch_services_even_with_custom_data() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("SaveScummer.app/Contents/MacOS/SaveScummer");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let mut command = bundled_host_command(&exe).unwrap();
        command.args(["--minimized", "--data-dir", "/tmp/dev"]);
        assert_eq!(command.get_program(), "/usr/bin/open");
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            [
                "-n",
                "-g",
                "-j",
                "-a",
                root.path().join("SaveScummer.app").to_str().unwrap(),
                "--args",
                "--minimized",
                "--data-dir",
                "/tmp/dev"
            ]
        );
        assert!(bundled_host_command(root.path().join("savescummer-host").as_path()).is_none());
    }
}

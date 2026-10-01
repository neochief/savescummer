//! Linux, glibc 2.35+, x86_64 and aarch64 (PLAN-BUILD.md Linux): one
//! AppImage that acts as all three programs, for the build machine's
//! architecture.
//!
//! The UI's GTK and WebKit come from Tauri's own AppImage bundling, which
//! solves what bundling WebKit needs (its helper processes, GTK's modules and
//! settings). xtask takes the AppDir it makes, puts the host and CLI beside
//! the UI, and replaces its entry point with `packaging/linux/AppRun`, so the
//! host starts and the bundled libraries reach only the UI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::naming::{self, CLI, HOST, UI};
use crate::package::{self, Inputs, Layout};
use crate::paths::{self, Mode};
use crate::{cmd, pins, setup};

pub use crate::unix::{ask_to_close, hide_window, spawn_detached};

#[cfg(target_arch = "x86_64")]
pub const PLATFORM: naming::Platform = naming::LINUX_X86_64;
#[cfg(target_arch = "x86_64")]
pub const RUST_TARGET: &str = "x86_64-unknown-linux-gnu";
#[cfg(target_arch = "aarch64")]
pub const PLATFORM: naming::Platform = naming::LINUX_AARCH64;
#[cfg(target_arch = "aarch64")]
pub const RUST_TARGET: &str = "aarch64-unknown-linux-gnu";
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("Linux builds are x86_64 or aarch64 (PLAN-BUILD.md Linux)");

pub const QT_AQT_HOST: &str = "linux";
pub const QT_AQT_ARCH: &str = "linux_gcc_64";
pub const QT_KIT_DIR: &str = "gcc_64";

/// The UI's real executable in the AppDir, as Tauri names it; `SaveScummer.UI`
/// beside it is the wrapper that sets up its libraries.
const TAURI_UI: &str = "savescummer-ui";
/// The icon name in the desktop entry and the icon theme.
const ICON: &str = "savescummer";
/// The desktop entry, named for the app id (the host's own copy in
/// `menu_entry.rs` and the shortcuts portal use the same id).
const DESKTOP_ENTRY: &str = "com.savescummer.SaveScummer.desktop";

pub fn package_name() -> String {
    "SaveScummer.AppDir".into()
}

pub fn check_build_machine() -> anyhow::Result<()> {
    Ok(())
}

/// Where the program with the fixed executable name `name` is in a package
/// (in the AppDir; the AppImage itself dispatches through `AppRun`).
pub fn program(package: &Path, name: &str) -> PathBuf {
    package.join("usr").join("bin").join(name)
}

/// Tauri's tool cache: `$XDG_CACHE_HOME/tauri`, pointed here for its builds.
fn tauri_cache() -> PathBuf {
    paths::tools().join("tauri-cache")
}

fn appimage_tools() -> PathBuf {
    paths::tools().join("appimage")
}

fn tool_path(tool: &pins::Pinned) -> PathBuf {
    if tool.tauri { tauri_cache().join("tauri").join(tool.file) } else { appimage_tools().join(tool.file) }
}

/// `setup linux-tools`: every pinned tool, checked against its SHA-256 when
/// downloaded. Tauri edits its tools in place once it runs them (their
/// AppImage header), so a tool already present counts as installed.
pub fn setup_linux_tools() -> anyhow::Result<()> {
    for tool in &pins::LINUX_TOOLS {
        let path = tool_path(tool);
        if path.is_file() {
            println!("{} is already installed", paths::show(&path));
            continue;
        }
        let downloaded = setup::download(tool.url, tool.sha256, tool.file)?;
        fs::create_dir_all(path.parent().expect("tools live in a folder"))?;
        fs::rename(&downloaded, &path)
            .or_else(|_| fs::copy(&downloaded, &path).map(|_| ()))
            .with_context(|| format!("installing {}", paths::show(&path)))?;
        make_executable(&path)?;
    }
    Ok(())
}

fn check_tools() -> anyhow::Result<()> {
    let missing: Vec<String> =
        pins::LINUX_TOOLS.iter().map(tool_path).filter(|p| !p.is_file()).map(|p| paths::show(&p)).collect();
    if !missing.is_empty() {
        bail!("{} missing — run `cargo xtask setup linux-tools`", missing.join(", "));
    }
    Ok(())
}

fn make_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("making {} executable", paths::show(path)))
}

/// Packaging builds Tauri's AppImage for its AppDir, from the pinned tools
/// only (Tauri downloads nothing it finds in its cache). Other builds make
/// no bundle.
pub fn tauri_bundle(build: &mut Command, package: bool) -> anyhow::Result<()> {
    if !package {
        build.arg("--no-bundle");
        return Ok(());
    }
    check_tools()?;
    build
        .args(["--bundles", "appimage", "--config", r#"{"bundle":{"active":true}}"#])
        .env("XDG_CACHE_HOME", tauri_cache())
        // Its tools are AppImages; this runs them without FUSE (CI runners).
        .env("APPIMAGE_EXTRACT_AND_RUN", "1");
    Ok(())
}

/// Stages the AppDir Tauri made as `<install>/AppDir`. Its AppImage isn't
/// used: xtask makes the release file from the finished package.
pub fn stage_tauri_bundle(mode: Mode, install: &Path) -> anyhow::Result<()> {
    let bundle = mode.cargo_out().join("bundle").join("appimage");
    let appdir = bundle.join("SaveScummer.AppDir");
    anyhow::ensure!(appdir.is_dir(), "the Tauri build did not create {}", paths::show(&appdir));
    copy_tree(&appdir, &install.join("AppDir"))?;
    for entry in fs::read_dir(&bundle)?.flatten() {
        if entry.path().extension().is_some_and(|ext| ext == "AppImage") {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Copies a folder as it is, links included (the AppDir links GTK's modules).
fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    if to.exists() {
        fs::remove_dir_all(to).with_context(|| format!("removing {}", to.display()))?;
    }
    fs::create_dir_all(to)?;
    cmd::run(Command::new("cp").arg("-a").arg(from.join(".")).arg(to))
}

/// ```text
/// AppRun                               packaging/linux/AppRun: host, `ui` or `cli`
/// com.savescummer.SaveScummer.desktop, savescummer.svg, .DirIcon
/// apprun-hooks/                        linuxdeploy's GTK setup, for the UI
/// usr/bin/SaveScummer                  host
/// usr/bin/SaveScummer.CLI              CLI
/// usr/bin/SaveScummer.UI               packaging/linux/SaveScummer.UI: the UI's wrapper
/// usr/bin/savescummer-ui               the Tauri UI
/// usr/lib/                             GTK, WebKit and their helpers
/// usr/share/applications/com.savescummer.SaveScummer.desktop, usr/share/icons/…
/// usr/share/savescummer/               licenses, manifest and checksums
/// ```
pub fn fill_package(root: &Path, inputs: &Inputs) -> anyhow::Result<Layout> {
    let ui = inputs.ui.context("Linux packages require the Tauri UI")?;
    let appdir = ui.install.join("AppDir");
    anyhow::ensure!(appdir.is_dir(), "{} is missing — the UI was built without packaging", paths::show(&appdir));
    copy_tree(&appdir, root)?;

    // Tauri's entry point and top-level files name its UI; ours start the host.
    for name in [
        "AppRun",
        "AppRun.wrapped",
        ".DirIcon",
        "SaveScummer.desktop",
        "SaveScummer.png",
        "savescummer-ui.png",
        "usr/share/applications/SaveScummer.desktop",
    ] {
        let path = root.join(name);
        if path.symlink_metadata().is_ok() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    let linux = paths::packaging().join("linux");
    package::copy_file(&inputs.host, &program(root, HOST))?;
    package::copy_file(&inputs.cli, &program(root, CLI))?;
    package::copy_file(&linux.join("SaveScummer.UI"), &program(root, UI))?;
    package::copy_file(&linux.join("AppRun"), &root.join("AppRun"))?;
    for path in [program(root, HOST), program(root, CLI), program(root, UI), root.join("AppRun")] {
        make_executable(&path)?;
    }
    anyhow::ensure!(program(root, TAURI_UI).is_file(), "the Tauri AppDir has no usr/bin/{TAURI_UI}");
    anyhow::ensure!(root.join("apprun-hooks").is_dir(), "the Tauri AppDir has no apprun-hooks/");

    let desktop = linux.join(DESKTOP_ENTRY);
    package::copy_file(&desktop, &root.join(DESKTOP_ENTRY))?;
    package::copy_file(&desktop, &root.join("usr/share/applications").join(DESKTOP_ENTRY))?;
    let svg = paths::root().join("assets").join("icon.svg");
    package::copy_file(&svg, &root.join(format!("{ICON}.svg")))?;
    package::copy_file(&svg, &root.join(format!("usr/share/icons/hicolor/scalable/apps/{ICON}.svg")))?;
    // Tauri's rendered PNGs (the sizes its config lists), under the icon's
    // name; `.DirIcon` is a PNG, 256×256 when there is one.
    let icons = root.join("usr/share/icons/hicolor");
    let mut pngs = Vec::new();
    for size in fs::read_dir(&icons)?.flatten() {
        let apps = size.path().join("apps");
        let tauri_png = apps.join(format!("{TAURI_UI}.png"));
        if tauri_png.is_file() {
            let png = apps.join(format!("{ICON}.png"));
            fs::rename(&tauri_png, &png)?;
            pngs.push((size.file_name() != "256x256", png));
        }
    }
    pngs.sort();
    let (_, png) = pngs.first().context("the Tauri AppDir has no PNG icon")?;
    package::copy_file(png, &root.join(".DirIcon"))?;

    let resources = PathBuf::from("usr/share/savescummer");
    Ok(Layout {
        resources: root.join(resources),
        required: vec![
            PathBuf::from("AppRun"),
            PathBuf::from(DESKTOP_ENTRY),
            PathBuf::from(format!("{ICON}.svg")),
            PathBuf::from(".DirIcon"),
            PathBuf::from("usr/bin").join(HOST),
            PathBuf::from("usr/bin").join(CLI),
            PathBuf::from("usr/bin").join(UI),
            PathBuf::from("usr/bin").join(TAURI_UI),
        ],
        unsummed: Vec::new(),
    })
}

/// Checks that every program is built for this machine's architecture.
pub fn finish_package(root: &Path) -> anyhow::Result<()> {
    let arch = match PLATFORM.arch {
        "x86_64" => "x86-64",
        _ => "aarch64",
    };
    for name in [HOST, CLI, TAURI_UI] {
        let binary = program(root, name);
        let kind = cmd::output(Command::new("file").arg("-b").arg(&binary))?;
        anyhow::ensure!(kind.contains(arch), "{} must be {arch}, found: {kind}", binary.display());
    }
    Ok(())
}

/// `appimagetool` with the pinned runtime: `dist/SaveScummer-linux-<arch>-<ver>.AppImage`.
pub fn release_file(package: &Path, version: &str) -> anyhow::Result<PathBuf> {
    check_tools()?;
    let out = paths::dist().join(PLATFORM.release_file(version));
    let tools = appimage_tools();
    cmd::run(
        Command::new(tools.join("appimagetool"))
            .arg("--no-appstream")
            .arg("--runtime-file")
            .arg(tools.join("runtime"))
            .arg(package)
            .arg(&out)
            .env("ARCH", PLATFORM.arch)
            .env("APPIMAGE_EXTRACT_AND_RUN", "1"),
    )?;
    anyhow::ensure!(out.is_file(), "appimagetool did not create {}", paths::show(&out));
    Ok(out)
}

pub fn setup_inno() -> anyhow::Result<()> {
    bail!("`setup inno` is only for Windows builds")
}

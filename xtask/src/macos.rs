//! macOS 13+, Apple Silicon (PLAN-BUILD.md macOS, PLAN-MACOS.md BUILD AND
//! PACKAGING): `SaveScummer.app`, ad-hoc signed, in a drag-to-Applications DMG.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::naming::{self, CLI, HOST, UI};
use crate::package::{self, Inputs, Layout};
use crate::{cmd, frontend, paths, pins};

pub use crate::unix::{ask_to_close, hide_window, spawn_detached};

pub const PLATFORM: naming::Platform = naming::MACOS;
pub const RUST_TARGET: &str = "aarch64-apple-darwin";
pub const QT_AQT_HOST: &str = "mac";
pub const QT_AQT_ARCH: &str = "clang_64";
pub const QT_KIT_DIR: &str = "macos";

/// The login agent the host registers with `SMAppService.agent` (PLAN-MACOS.md
/// LAUNCH AT LOGIN).
const LOGIN_AGENT: &str = "com.savescummer.SaveScummer.host.plist";

pub fn package_name() -> String {
    "SaveScummer.app".into()
}

/// Intel Macs aren't supported.
pub fn check_build_machine() -> anyhow::Result<()> {
    if std::env::consts::ARCH != "aarch64" {
        bail!("SaveScummer builds only on Apple Silicon Macs; Intel Macs aren't supported");
    }
    Ok(())
}

pub fn cmake_args() -> Vec<String> {
    vec!["-DCMAKE_OSX_ARCHITECTURES=arm64".into(), format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={}", pins::MIN_MACOS)]
}

pub fn qt_runtime_env(_command: &mut Command, _kit: &Path) {}

/// Where the program with the fixed executable name `name` is in a package.
pub fn program(package: &Path, name: &str) -> PathBuf {
    package.join("Contents").join("MacOS").join(name)
}

/// ```text
/// Contents/Info.plist
/// Contents/MacOS/SaveScummer        host: the main executable, what opening the app runs
/// Contents/MacOS/SaveScummer.UI     UI, with Qt in Contents/Frameworks (once it exists)
/// Contents/MacOS/SaveScummer.CLI    CLI
/// Contents/Library/LaunchAgents/com.savescummer.SaveScummer.host.plist
/// Contents/Resources/SaveScummer.icns, then licenses, manifest and checksums
/// ```
///
/// Ends by signing the bundle once, so the nested code already carries its
/// final signature when SHA256SUMS.txt is written; `finish_package` signs again.
pub fn fill_package(root: &Path, inputs: &Inputs) -> anyhow::Result<Layout> {
    let contents = PathBuf::from("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    let agent = contents.join("Library").join("LaunchAgents").join(LOGIN_AGENT);
    let icns = resources.join("SaveScummer.icns");
    let mut required =
        vec![contents.join("Info.plist"), macos.join(HOST), macos.join(CLI), agent.clone(), icns.clone()];

    package::copy_file(&inputs.host, &program(root, HOST))?;
    package::copy_file(&inputs.cli, &program(root, CLI))?;
    if let Some(ui) = inputs.ui {
        deploy_ui(root, &ui.install)?;
        required.push(macos.join(UI));
    }

    let plist = fs::read_to_string(paths::packaging().join("macos").join("Info.plist.in"))
        .context("reading packaging/macos/Info.plist.in")?
        .replace("{version}", inputs.version)
        .replace("{min_macos}", pins::MIN_MACOS);
    fs::write(root.join(&contents).join("Info.plist"), plist).context("writing Info.plist")?;
    package::copy_file(&paths::packaging().join("macos").join(LOGIN_AGENT), &root.join(&agent))?;
    make_icns(&root.join(&icns))?;

    sign(root)?;
    Ok(Layout {
        resources: root.join(resources),
        required,
        // The last signing rewrites both: the main executable's signature
        // seals every resource, SHA256SUMS.txt included.
        unsummed: vec![macos.join(HOST), contents.join("_CodeSignature")],
    })
}

/// Signs the finished bundle (ad-hoc: Apple Silicon runs nothing unsigned)
/// and checks it the way macOS will.
///
/// Signing order: `fill_package` already signed everything once, and ad-hoc
/// signing is deterministic, so this pass leaves nested code (the CLI, the UI,
/// Qt) byte for byte as summed. Only the main executable and
/// `_CodeSignature/` change, since they seal SHA256SUMS.txt itself; they're
/// left out of it, and the signature covers them instead.
pub fn finish_package(root: &Path) -> anyhow::Result<()> {
    sign(root)?;
    cmd::run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(root))
}

fn sign(bundle: &Path) -> anyhow::Result<()> {
    cmd::run(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(bundle))
}

/// The UI next to the host, and the Qt it needs deployed by `macdeployqt`.
/// Expects the UI's `cmake --install` to put a plain executable in `bin/`.
fn deploy_ui(root: &Path, install: &Path) -> anyhow::Result<()> {
    let ui = program(root, UI);
    package::copy_file(&install.join("bin").join(UI), &ui)?;
    let macdeployqt = cmd::tool(
        &frontend::kit().join("bin").join("macdeployqt"),
        &format!("macdeployqt (Qt {})", pins::QT_VERSION),
        "qt",
    )?;
    let mut command = Command::new(macdeployqt);
    command.arg(root).arg(format!("-executable={}", ui.display()));
    cmd::run(&mut command)
}

/// The full-color Dock and Finder icon, rendered from `assets/icon.svg` at
/// every size an `.icns` holds. (The menu-bar template in `assets/macos/` is
/// the host's own.)
fn make_icns(out: &Path) -> anyhow::Result<()> {
    let rsvg = cmd::on_path("rsvg-convert", "install it with `brew install librsvg` (it renders the app icon)")?;
    let svg = paths::root().join("assets").join("icon.svg");
    let iconset = paths::scratch().join("SaveScummer.iconset");
    if iconset.exists() {
        fs::remove_dir_all(&iconset).with_context(|| format!("removing {}", iconset.display()))?;
    }
    fs::create_dir_all(&iconset)?;
    for size in [16, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let pixels = (size * scale).to_string();
            let png = iconset.join(format!("icon_{size}x{size}{suffix}.png"));
            cmd::output(Command::new(&rsvg).args(["-w", &pixels, "-h", &pixels, "-o"]).arg(&png).arg(&svg))?;
        }
    }
    fs::create_dir_all(out.parent().expect("inside the bundle"))?;
    cmd::output(Command::new("iconutil").args(["-c", "icns", "-o"]).arg(out).arg(&iconset))?;
    Ok(())
}

/// Makes the DMG in `dist/`: the app next to an `Applications` link, so
/// installing is a drag.
pub fn release_file(package: &Path, version: &str) -> anyhow::Result<PathBuf> {
    let folder = paths::scratch().join("dmg");
    if folder.exists() {
        fs::remove_dir_all(&folder).with_context(|| format!("removing {}", folder.display()))?;
    }
    fs::create_dir_all(&folder)?;
    // ditto keeps the bundle exactly as signed: symlinks, modes, attributes.
    cmd::run(Command::new("ditto").arg(package).arg(folder.join(package_name())))?;
    std::os::unix::fs::symlink("/Applications", folder.join("Applications")).context("linking Applications")?;

    let out = paths::dist().join(PLATFORM.release_file(version));
    let mut command = Command::new("hdiutil");
    command.args(["create", "-volname", "SaveScummer", "-format", "UDZO", "-ov", "-srcfolder"]).arg(&folder).arg(&out);
    cmd::run(&mut command)?;
    let _ = fs::remove_dir_all(&folder);
    Ok(out)
}

pub fn setup_inno() -> anyhow::Result<()> {
    bail!("`setup inno` is only for Windows builds")
}

pub fn setup_linux_tools() -> anyhow::Result<()> {
    bail!("`setup linux-tools` is only for Linux builds")
}

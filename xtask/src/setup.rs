//! `cargo xtask setup …` (PLAN-BUILD.md TOOLCHAINS): the only way tools get
//! onto the machine. Each installs a pinned version into `.runtime/` and is
//! safe to re-run; nothing is ever downloaded silently by other commands.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

use crate::{cmd, frontend, naming, paths, pins, platform};

/// Downloads `url` into `build/tmp/<name>` and checks its SHA-256.
// Only the Windows setup downloads anything yet (`setup linux-tools` will).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn download(url: &str, sha256: &str, name: &str) -> anyhow::Result<PathBuf> {
    println!("downloading {url}");
    let mut bytes = Vec::new();
    ureq::get(url)
        .call()
        .with_context(|| format!("downloading {url}"))?
        .into_body()
        .with_config()
        .limit(1 << 30)
        .reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("downloading {url}"))?;
    let actual = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    if !actual.eq_ignore_ascii_case(sha256) {
        bail!("{url} has SHA-256 {actual}, but the pin is {sha256}; not using it");
    }
    let dir = paths::scratch();
    fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

pub fn inno() -> anyhow::Result<()> {
    platform::setup_inno()
}

pub fn linux_tools() -> anyhow::Result<()> {
    platform::setup_linux_tools()
}

/// The pinned cargo-about, built by `cargo install --locked`.
pub fn cargo_about() -> anyhow::Result<()> {
    let root = paths::tools().join("cargo-about");
    let exe = root.join("bin").join(naming::exe("cargo-about"));
    if exe.is_file()
        && cmd::output(Command::new(&exe).arg("--version"))
            .is_ok_and(|v| v.split_whitespace().last() == Some(pins::CARGO_ABOUT_VERSION))
    {
        println!("cargo-about {} is already installed", pins::CARGO_ABOUT_VERSION);
        return Ok(());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    cmd::run(
        Command::new(cargo)
            .args(["install", "cargo-about", "--locked", "--force", "--features", "cli"])
            .args(["--version", pins::CARGO_ABOUT_VERSION])
            .arg("--root")
            .arg(&root),
    )
}

/// The pinned Qt kit, via aqtinstall in a venv (needs Python 3.9+).
pub fn qt() -> anyhow::Result<()> {
    platform::check_build_machine()?;
    let kit = frontend::kit();
    if kit.join("lib").is_dir() {
        println!("Qt {} is already installed in {}", pins::QT_VERSION, paths::show(&kit));
        return Ok(());
    }
    let venv = paths::tools().join("aqtinstall");
    let bin = venv.join(if cfg!(windows) { "Scripts" } else { "bin" });
    let python = bin.join(naming::exe("python"));
    let aqt = bin.join(naming::exe("aqt"));
    if !python.is_file() {
        let system = ["python3", "python"]
            .iter()
            .filter_map(|name| cmd::find_on_path(name))
            .find(|p| cmd::succeeds(Command::new(p).args(["-c", "import sys; sys.exit(sys.version_info < (3, 9))"])))
            .context("Python 3.9+ not found on PATH — install it from python.org (it's only needed to install Qt)")?;
        cmd::run(Command::new(system).args(["-m", "venv"]).arg(&venv))?;
    }
    let wanted = format!("aqtinstall=={}", pins::AQTINSTALL_VERSION);
    cmd::run(Command::new(&python).args(["-m", "pip", "install", "--disable-pip-version-check", "--quiet", &wanted]))?;
    cmd::run(
        Command::new(&aqt)
            .args(["install-qt", platform::QT_AQT_HOST, "desktop", pins::QT_VERSION, platform::QT_AQT_ARCH])
            .arg("--outputdir")
            .arg(paths::runtime().join("Qt")),
    )?;
    if !kit.join("lib").is_dir() {
        bail!("aqtinstall finished but {} is missing", paths::show(&kit));
    }
    println!("installed Qt {} into {}", pins::QT_VERSION, paths::show(&kit));
    Ok(())
}

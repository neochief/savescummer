//! `test`, `build` and `dist` (PLAN-BUILD.md XTASK, TEST).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
use clap::ValueEnum;

use crate::naming::{self, CARGO_CLI, CARGO_HOST};
use crate::package::{self, Inputs};
use crate::paths::{self, Mode};
use crate::{cache, catalog, cmd, frontend, platform, procs, version};

/// What a build produced.
pub struct Built {
    pub version: String,
    /// The APP PACKAGE, when one was assembled.
    pub package: Option<PathBuf>,
}

/// Cargo. Its settings for every build (the minimum macOS) live in
/// `.cargo/config.toml`, so plain cargo builds agree with ours.
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Broad development test sections. Bare `test` is always the full gate.
#[derive(Clone, Copy, ValueEnum)]
pub enum TestSection {
    /// Rust workspace packages except the end-to-end harness.
    Crates,
    /// Host and CLI end-to-end tests.
    E2e,
    /// UI typecheck and headless tests.
    Ui,
}

pub fn build(mode: Mode, package: bool) -> anyhow::Result<Built> {
    let version = version::current()?;
    platform::check_build_machine()?;
    if !frontend::present() {
        bail!("the Tauri UI source is missing from {}", paths::show(&frontend::source()));
    }
    // A clean rebuild: nothing keeps running from the outputs we're replacing.
    procs::stop_outputs()?;

    // Release builds must match Cargo.lock exactly; dev builds may refresh it.
    let locked: &[&str] = if mode == Mode::Release { &["--locked"] } else { &[] };
    let mut rust = cargo();
    rust.args(["build", "-p", CARGO_HOST, "-p", CARGO_CLI]).args(locked);
    if mode == Mode::Release {
        // Only release builds may create a sign-in entry (PLAN-BUILD.md WHAT
        // THE APP MUST PROVIDE); dev hosts refuse.
        rust.arg("--release").env("SAVESCUMMER_RELEASE_BUILD", "1");
    }
    let out = mode.cargo_out();
    let host = out.join(naming::exe(CARGO_HOST));
    let cli = out.join(naming::exe(CARGO_CLI));
    let rust_inputs = || {
        let mut key = cache::Key::new("rust")?;
        key.cargo_setup()?;
        for bin in [CARGO_HOST, CARGO_CLI] {
            key.dep_info(&out.join(format!("{bin}.d")), &[])?;
        }
        Ok(key)
    };
    cache::Step::new("rust", mode, true, vec![host.clone(), cli.clone()]).run(rust_inputs, || cmd::run(&mut rust))?;

    let ui = if frontend::present() {
        Some(frontend::build(mode, &version, package)?)
    } else {
        println!("UI: {} doesn't exist yet; building the host and CLI only", paths::show(&frontend::source()));
        None
    };

    let package = if package {
        Some(package::assemble(&Inputs { mode, version: &version, host, cli, ui: ui.as_ref() })?)
    } else {
        None
    };
    Ok(Built { version, package })
}

/// Release build and this platform's release file, without running tests.
pub fn dist() -> anyhow::Result<()> {
    let built = build(Mode::Release, true)?;
    let package = built.package.expect("release builds always package");
    let dist = paths::dist();
    if dist.exists() {
        fs::remove_dir_all(&dist).with_context(|| format!("emptying {}", dist.display()))?;
    }
    fs::create_dir_all(&dist)?;
    let file = platform::release_file(&package, &built.version)?;

    let expected = platform::PLATFORM.release_file(&built.version);
    let found: Vec<String> =
        fs::read_dir(&dist)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    if found != [expected.clone()] {
        bail!("dist/ should hold exactly {expected}, but has: {}", found.join(", "));
    }
    println!("release file: {} ({})", paths::show(&file), package::sha256_file(&file)?);
    Ok(())
}

/// The full quality gate, or a broad development test section.
pub fn test(section: Option<TestSection>) -> anyhow::Result<()> {
    match section {
        Some(TestSection::Crates) => {
            procs::stop_outputs()?;
            test_crates()
        }
        Some(TestSection::E2e) => {
            procs::stop_outputs()?;
            test_e2e()
        }
        Some(TestSection::Ui) => frontend::test(),
        None => test_all(),
    }
}

fn test_all() -> anyhow::Result<()> {
    procs::stop_outputs()?;
    cmd::run(cargo().args(["fmt", "--all", "--check"]))?;
    cmd::run(cargo().args(["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]))?;
    // Keep the full Rust run in one Cargo invocation. Separate invocations
    // unify features differently and rebuild shared dependencies twice.
    cmd::run(cargo().args(["test", "--workspace", "--locked", "--", "--test-threads=1"]))?;
    // Not xtask itself: workspace-wide feature unification would relink the
    // running xtask.exe, which Windows can't replace. It's already built, and
    // clippy and the tests above cover it.
    cmd::run(cargo().args(["build", "--workspace", "--exclude", "xtask", "--locked"]))?;
    catalog::check()?;
    frontend::test()
}

fn test_crates() -> anyhow::Result<()> {
    cmd::run(cargo().args([
        "test",
        "--workspace",
        "--exclude",
        "savescummer-e2e",
        "--locked",
        "--",
        "--test-threads=1",
    ]))
}

fn test_e2e() -> anyhow::Result<()> {
    cmd::run(cargo().args(["test", "-p", "savescummer-e2e", "--locked", "--", "--test-threads=1"]))
}

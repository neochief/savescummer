//! `build`, `dist` and `check` (PLAN-BUILD.md XTASK, CHECK).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

use crate::naming::{self, CARGO_CLI, CARGO_HOST};
use crate::package::{self, Inputs};
use crate::paths::{self, Mode};
use crate::{catalog, cmd, frontend, platform, procs, version};

pub struct Options {
    pub release: bool,
    pub test: bool,
    pub package: bool,
}

/// What a build produced.
pub struct Built {
    pub version: String,
    /// The APP PACKAGE, when one was assembled.
    pub package: Option<PathBuf>,
    pub has_desktop: bool,
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

pub fn build(options: &Options) -> anyhow::Result<Built> {
    let mode = if options.release { Mode::Release } else { Mode::Dev };
    let version = version::current()?;
    platform::check_build_machine()?;
    // A clean rebuild: nothing keeps running from the outputs we're replacing.
    procs::stop_outputs()?;

    // Release builds must match Cargo.lock exactly; dev builds may refresh it.
    let locked: &[&str] = if options.release { &["--locked"] } else { &[] };
    let mut rust = cargo();
    rust.args(["build", "-p", CARGO_HOST, "-p", CARGO_CLI]).args(locked);
    if options.release {
        // Only release builds may create a sign-in entry (PLAN-BUILD.md WHAT
        // THE APP MUST PROVIDE); dev hosts refuse.
        rust.arg("--release").env("SAVESCUMMER_RELEASE_BUILD", "1");
    }
    cmd::run(&mut rust)?;
    let out = mode.cargo_out();
    let host = out.join(naming::exe(CARGO_HOST));
    let cli = out.join(naming::exe(CARGO_CLI));

    if options.test {
        // Tests always build as dev: they check dev-only behavior too.
        cmd::run(cargo().args(["test", "--workspace"]).args(locked))?;
    }

    let desktop = if frontend::present() {
        let test_host = Mode::Dev.cargo_out().join(naming::exe(CARGO_HOST));
        if options.test && !test_host.is_file() {
            cmd::run(cargo().args(["build", "-p", CARGO_HOST]).args(locked))?;
        }
        Some(frontend::build(mode, &version, options.test, if options.test { &test_host } else { &host })?)
    } else {
        println!("desktop: {} doesn't exist yet; building the host and CLI only", paths::show(&frontend::source()));
        None
    };

    let package = if options.package || options.release {
        Some(package::assemble(&Inputs { mode, version: &version, host, cli, desktop: desktop.as_ref() })?)
    } else {
        None
    };
    Ok(Built { version, package, has_desktop: desktop.is_some() })
}

/// `build --release --test`, then the platform's one release file in `dist/`.
pub fn dist() -> anyhow::Result<()> {
    let built = build(&Options { release: true, test: true, package: true })?;
    let package = built.package.expect("release builds always package");
    let dist = paths::dist();
    if dist.exists() {
        fs::remove_dir_all(&dist).with_context(|| format!("emptying {}", dist.display()))?;
    }
    fs::create_dir_all(&dist)?;
    let file = platform::release_file(&package, &built.version, built.has_desktop)?;

    let expected = platform::PLATFORM.release_file(&built.version);
    let found: Vec<String> =
        fs::read_dir(&dist)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    if found != [expected.clone()] {
        bail!("dist/ should hold exactly {expected}, but has: {}", found.join(", "));
    }
    println!("release file: {} ({})", paths::show(&file), package::sha256_file(&file)?);
    Ok(())
}

/// The quality gate, stopping at the first failure.
pub fn check() -> anyhow::Result<()> {
    procs::stop_outputs()?;
    cmd::run(cargo().args(["fmt", "--all", "--check"]))?;
    cmd::run(cargo().args(["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]))?;
    cmd::run(cargo().args(["test", "--workspace", "--locked"]))?;
    // Not xtask itself: workspace-wide feature unification would relink the
    // running xtask.exe, which Windows can't replace. It's already built, and
    // clippy and the tests above cover it.
    cmd::run(cargo().args(["build", "--workspace", "--exclude", "xtask", "--locked"]))?;
    println!("> cargo xtask catalog --check");
    match catalog::run(true, false, Default::default())? {
        savescummer_catalog_build::RunResult::Ok => Ok(()),
        _ => bail!("the catalog check failed (see above); `cargo xtask catalog` regenerates it"),
    }
}

//! Repository tasks (`cargo xtask <task>`): the one build tool, the same on
//! every OS (PLAN-BUILD.md). CI only calls these, so a CI failure can always
//! be reproduced locally.
//!
//! Platform logic lives in `windows.rs`, `macos.rs` and `linux.rs`, each
//! compiled only on its OS; the shared modules never contain any.

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use savescummer_catalog_build::{Options, RunResult};

mod build;
mod catalog;
mod clean;
mod cmd;
mod frontend;
mod naming;
mod package;
mod paths;
mod pins;
mod procs;
mod release;
mod session;
mod setup;
mod version;

#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(unix)]
mod unix;

#[derive(Parser)]
#[command(about = "SaveScummer repository tasks")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// The quality gate: fmt, clippy, tests, build, catalog check.
    Check,
    /// Build the host, CLI and UI.
    Build {
        /// Optimized release build; always assembles the app package.
        #[arg(long)]
        release: bool,
        /// Also run the Rust and UI tests.
        #[arg(long)]
        test: bool,
        /// Assemble the app package under build/<mode>/package/.
        #[arg(long)]
        package: bool,
    },
    /// Dev build, then the dev host, which shows the UI.
    Run {
        /// Simulated games and operations.
        #[arg(long)]
        demo: bool,
        /// Gracefully stop other running hosts (e.g. the installed app's).
        #[arg(long)]
        stop_other_hosts: bool,
    },
    /// Start or stop just the dev host.
    Host {
        #[command(subcommand)]
        action: HostAction,
    },
    /// `build --release --test`, then this platform's release file in dist/.
    Dist,
    /// Stop output processes and remove build/ and dist/.
    Clean {
        /// Also remove target/.
        #[arg(long)]
        deep: bool,
    },
    /// Cut a release: bump the version, check, commit, tag and push.
    Release {
        /// The new version, e.g. 1.2.3 (a leading `v` is fine).
        version: String,
        /// Skip `cargo xtask check` (emergencies only).
        #[arg(long)]
        skip_checks: bool,
        /// Print the push commands instead of running them.
        #[arg(long)]
        no_push: bool,
    },
    /// Upload dist/ to the version's draft GitHub release.
    Publish,
    /// Install a pinned tool into .runtime/.
    Setup { tool: Tool },
    /// Regenerate catalog/catalog.json and print the build report.
    Catalog {
        /// Regenerate in memory and fail if the committed bundle differs (CI).
        #[arg(long)]
        check: bool,
        /// Also fail on warnings.
        #[arg(long)]
        strict: bool,
        /// Leave out `Keep` games that can't be built (not in the manifest
        /// or the addendum, or no usable save target) with a warning, instead
        /// of failing the build.
        #[arg(long)]
        allow_unbuildable: bool,
    },
}

#[derive(Subcommand)]
enum HostAction {
    /// Dev build, then start the dev host with .runtime/dev as its data.
    Start {
        /// Simulated games and operations.
        #[arg(long)]
        demo: bool,
    },
    /// Stop the recorded dev host gracefully.
    Stop,
}

#[derive(Clone, Copy, ValueEnum)]
enum Tool {
    Qt,
    Inno,
    LinuxTools,
    CargoAbout,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.task {
        Task::Catalog { check, strict, allow_unbuildable } => {
            return match catalog::run(check, strict, Options { allow_unbuildable }) {
                Ok(RunResult::Ok) => ExitCode::SUCCESS,
                Ok(_) => ExitCode::FAILURE,
                Err(e) => fail(e),
            };
        }
        Task::Check => build::check(),
        Task::Build { release, test, package } => build::build(&build::Options { release, test, package }).map(drop),
        Task::Run { demo, stop_other_hosts } => session::run(demo, stop_other_hosts),
        Task::Host { action: HostAction::Start { demo } } => session::host_start(demo),
        Task::Host { action: HostAction::Stop } => session::host_stop(),
        Task::Dist => build::dist(),
        Task::Clean { deep } => clean::clean(deep),
        Task::Release { version, skip_checks, no_push } => release::release(&version, skip_checks, no_push),
        Task::Publish => release::publish(),
        Task::Setup { tool } => match tool {
            Tool::Qt => setup::qt(),
            Tool::Inno => setup::inno(),
            Tool::LinuxTools => setup::linux_tools(),
            Tool::CargoAbout => setup::cargo_about(),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn fail(e: anyhow::Error) -> ExitCode {
    eprintln!("error: {e:#}");
    ExitCode::FAILURE
}

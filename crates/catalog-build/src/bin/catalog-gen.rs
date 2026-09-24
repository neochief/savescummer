//! `catalog-gen`: builds `catalog.json` from explicit input paths.
//! `cargo xtask catalog` is the everyday front end with the repo's paths.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use savescummer_catalog_build::{Options, Paths, RunResult, build_from_files, finish};

#[derive(Parser)]
#[command(about = "Build the SaveScummer catalog bundle from games.csv, the addendum and the pinned manifest")]
struct Args {
    #[arg(long)]
    games: PathBuf,
    #[arg(long)]
    addendum: PathBuf,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    lock: PathBuf,
    /// Where the bundle is written (or, with --check, compared).
    #[arg(long)]
    out: PathBuf,
    /// Where the JSON build report is written.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Regenerate in memory and fail if `--out` differs.
    #[arg(long)]
    check: bool,
    /// Also fail on warnings.
    #[arg(long)]
    strict: bool,
    /// Leave out `Keep` games that can't be built (not in the manifest or
    /// the addendum, or no usable save target) with a warning, instead of
    /// failing the build.
    #[arg(long)]
    allow_unbuildable: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let paths = Paths { games: args.games, addendum: args.addendum, manifest: args.manifest, lock: args.lock };
    let options = Options { allow_unbuildable: args.allow_unbuildable };
    let result = build_from_files(&paths, options)
        .and_then(|outcome| finish(&outcome, &args.out, args.report.as_deref(), args.check, args.strict));
    match result {
        Ok(RunResult::Ok) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

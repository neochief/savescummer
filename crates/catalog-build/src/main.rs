//! `catalog-gen`: regenerate `catalog/catalog.json` from the pinned manifest
//! and the two human inputs. Deterministic; suitable for CI dirty-diff checks.

use anyhow::{Context, Result, bail};
use clap::Parser;
use savescummer_catalog_build::{BuildError, Inputs, Lock, build, build_lenient};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Build the SaveScummer catalog bundle")]
struct Cli {
    #[arg(long, default_value = "catalog/games.csv")]
    games: PathBuf,
    #[arg(long, default_value = "catalog/addendum.yaml")]
    addendum: PathBuf,
    #[arg(long, default_value = ".runtime/catalog/manifest.yaml")]
    manifest: PathBuf,
    #[arg(long, default_value = "catalog/manifest.lock")]
    lock: PathBuf,
    #[arg(long, default_value = "catalog/catalog.json")]
    out: PathBuf,
    /// Verify the committed bundle is current without writing it.
    #[arg(long)]
    check: bool,
    /// Omit games with no usable save directory instead of failing (bootstrap).
    #[arg(long)]
    allow_partial: bool,
    /// Also write the machine-readable build report here.
    #[arg(long)]
    report: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let games = read(&cli.games)?;
    let addendum = read(&cli.addendum)?;
    let manifest = read(&cli.manifest)?;
    let lock = Lock::parse(&read(&cli.lock)?)
        .map_err(|e| anyhow::anyhow!("{} is invalid: {e}", cli.lock.display()))?;

    let output = match (if cli.allow_partial {
        build_lenient
    } else {
        build
    })(&Inputs {
        games_csv: &games,
        addendum_yaml: &addendum,
        manifest_yaml: &manifest,
        lock: &lock,
    }) {
        Ok(output) => output,
        Err(BuildError::Failed { report }) => {
            for warning in &report.warnings {
                eprintln!("warning: {warning}");
            }
            for error in &report.errors {
                eprintln!("error: {error}");
            }
            bail!("catalog build failed with {} error(s)", report.errors.len());
        }
        Err(error) => return Err(error.into()),
    };

    for warning in &output.report.warnings {
        eprintln!("warning: {warning}");
    }
    let json = output.bundle.to_json_pretty()?;
    if cli.check {
        let existing = std::fs::read_to_string(&cli.out).unwrap_or_default();
        if existing != json {
            bail!(
                "{} is stale; run `cargo run -p savescummer-catalog-build --bin catalog-gen`",
                cli.out.display()
            );
        }
    } else {
        write(&cli.out, &json)?;
    }
    if let Some(path) = &cli.report {
        write(path, &serde_json::to_string_pretty(&output.report)?)?;
    }
    println!(
        "catalog: {} games, {} warning(s), source revision {}",
        output.report.games,
        output.report.warnings.len(),
        output.bundle.source.revision
    );
    Ok(())
}

fn read(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}

/// Write a build artifact, creating its directory when missing.
fn write(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("cannot write {}", path.display()))
}

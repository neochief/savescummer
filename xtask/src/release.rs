//! `release <version>` and `publish` (PLAN-BUILD.md RELEASING). Tooling only
//! ever makes draft releases; a human publishes them.

use std::fs;
use std::process::Command;

use anyhow::{Context, bail};

use crate::{build, cmd, naming, paths, version};

fn git() -> Command {
    let mut command = Command::new("git");
    command.current_dir(paths::root());
    command
}

fn gh() -> Command {
    let mut command = Command::new("gh");
    command.current_dir(paths::root());
    command
}

/// Bumps the version, runs the checks, commits, tags and pushes.
pub fn release(input: &str, skip_checks: bool, no_push: bool) -> anyhow::Result<()> {
    let version = version::parse(input)?;
    let tag = version::tag(&version);
    let branch = cmd::output(git().args(["symbolic-ref", "--short", "-q", "HEAD"]))
        .context("not on a branch — check out the branch to release from")?;
    if !cmd::output(git().args(["status", "--porcelain"]))?.is_empty() {
        bail!("the working tree has changes — commit or stash them first");
    }
    let current = version::current()?;
    if version == current {
        bail!("the version is already {current}");
    }
    if cmd::succeeds(git().args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")])) {
        bail!("tag {tag} already exists locally");
    }
    if !cmd::output(git().args(["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")]))?.is_empty() {
        bail!("tag {tag} already exists on origin");
    }

    println!("releasing {version} (was {current}) from {branch}");
    version::set(&version)?;
    let prepared = cmd::run(
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())).args(["update", "--workspace"]),
    )
    .and_then(|()| if skip_checks { Ok(()) } else { build::check() });
    if let Err(e) = prepared {
        // The tree was clean, so restoring these two leaves it clean again.
        let _ = cmd::run(git().args(["checkout", "--", "Cargo.toml", "Cargo.lock"]));
        return Err(e.context(format!("release {version} abandoned; the version bump was undone")));
    }

    cmd::run(git().args(["commit", "-m", &format!("Release {version}"), "--", "Cargo.toml", "Cargo.lock"]))?;
    cmd::run(git().args(["tag", "-a", &tag, "-m", &format!("Release {version}")]))?;
    let push_branch = format!("git push origin {branch}");
    let push_tag = format!("git push origin {tag}");
    if no_push {
        println!("not pushed; when ready, run:\n  {push_branch}\n  {push_tag}");
    } else {
        cmd::run(git().args(["push", "origin", &branch]))?;
        cmd::run(git().args(["push", "origin", &tag]))?;
        println!("pushed {tag}; the release workflow builds the draft release");
    }
    Ok(())
}

/// Uploads `dist/` to the draft GitHub release for the current version.
pub fn publish() -> anyhow::Result<()> {
    let version = version::current()?;
    let tag = version::tag(&version);

    if !cmd::succeeds(gh().arg("--version")) {
        bail!("gh not found — install the GitHub CLI from https://cli.github.com");
    }
    if !cmd::succeeds(gh().args(["auth", "status"])) {
        bail!("gh isn't logged in — run `gh auth login` (CI sets GH_TOKEN)");
    }
    cmd::output(git().args(["remote", "get-url", "origin"])).context("no `origin` remote")?;
    let tagged = cmd::output(git().args(["rev-parse", &format!("refs/tags/{tag}^{{commit}}")]))
        .with_context(|| format!("tag {tag} doesn't exist — cut the release with `cargo xtask release {version}`"))?;
    let head = cmd::output(git().args(["rev-parse", "HEAD"]))?;
    if tagged != head {
        bail!("tag {tag} doesn't point at HEAD — check out {tag} first");
    }

    let dist = paths::dist();
    let expected: Vec<String> = naming::ALL.iter().filter(|p| p.ships).map(|p| p.release_file(&version)).collect();
    let mut found: Vec<String> = fs::read_dir(&dist)
        .with_context(|| format!("{} doesn't exist — run `cargo xtask dist`", paths::show(&dist)))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut wanted = expected.clone();
    wanted.sort();
    if found != wanted {
        bail!("dist/ must hold exactly {} for {version}, but has: {}", wanted.join(", "), found.join(", "));
    }
    let files: Vec<_> = expected.iter().map(|f| dist.join(f)).collect();

    let state = cmd::output(gh().args(["release", "view", &tag, "--json", "isDraft", "--jq", ".isDraft"]));
    match state.as_deref() {
        Ok("false") => {
            bail!("release {tag} is already published; a published release never changes — cut a new version")
        }
        Ok(_) => {
            cmd::run(gh().args(["release", "upload", &tag, "--clobber"]).args(&files))?;
        }
        Err(_) => {
            cmd::run(
                gh().args(["release", "create", &tag, "--draft", "--verify-tag", "--generate-notes"])
                    .args(["--title", &format!("SaveScummer {version}")])
                    .args(&files),
            )?;
        }
    }
    let url = cmd::output(gh().args(["release", "view", &tag, "--json", "url", "--jq", ".url"]))?;
    println!("draft release: {url}");
    Ok(())
}

//! Running external programs, with failures that say what to run next.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

/// Runs `command` with inherited output and fails unless it succeeds.
pub fn run(command: &mut Command) -> anyhow::Result<()> {
    println!("> {}", describe(command));
    let status = command.status().with_context(|| format!("couldn't start {}", program_name(command)))?;
    if !status.success() {
        bail!("{} failed ({status})", describe(command));
    }
    Ok(())
}

/// Runs `command` quietly and returns its trimmed standard output.
pub fn output(command: &mut Command) -> anyhow::Result<String> {
    let output = command.output().with_context(|| format!("couldn't start {}", program_name(command)))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{} failed ({}): {}", describe(command), output.status, stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Runs `command` quietly and reports only whether it succeeded.
pub fn succeeds(command: &mut Command) -> bool {
    command.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Finds `name` on `PATH`, or fails with `hint`.
pub fn on_path(name: &str, hint: &str) -> anyhow::Result<PathBuf> {
    find_on_path(name).with_context(|| format!("{name} not found on PATH — {hint}"))
}

pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(&exe)).find(|p| p.is_file())
}

/// `path` must exist, or the error names the setup command that makes it.
pub fn tool(path: &Path, what: &str, setup: &str) -> anyhow::Result<PathBuf> {
    if path.is_file() { Ok(path.to_path_buf()) } else { bail!("{what} not found — run `cargo xtask setup {setup}`") }
}

fn program_name(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

fn describe(command: &Command) -> String {
    let mut text = program_name(command);
    for arg in command.get_args() {
        let arg = arg.to_string_lossy();
        if arg.contains(' ') {
            text.push_str(&format!(" \"{arg}\""));
        } else {
            text.push(' ');
            text.push_str(&arg);
        }
    }
    text
}

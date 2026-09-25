//! `clean [--deep]`: stop output processes, remove `build/` and `dist/`
//! (`--deep` also `target/`). `.runtime/` holds SDKs and dev data and is
//! never touched. Needs no Qt or other tool, so a broken setup can always be
//! cleaned.

use std::fs;
use std::path::Path;

use anyhow::Context;

use crate::{paths, session};

pub fn clean(deep: bool) -> anyhow::Result<()> {
    session::stop_all_output()?;
    remove(&paths::build())?;
    remove(&paths::dist())?;
    if deep {
        release_own_executable();
        remove(&paths::target())?;
    }
    Ok(())
}

fn remove(dir: &Path) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    println!("removing {}", paths::show(dir));
    fs::remove_dir_all(dir).with_context(|| format!("removing {} (is something still using it?)", dir.display()))
}

/// Windows can't delete a running executable, but can move it: xtask itself
/// runs from `target/`, so it steps out of the way before `target/` goes.
fn release_own_executable() {
    if !cfg!(windows) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    if !crate::procs::is_under(&exe, &paths::target()) {
        return;
    }
    let aside = std::env::temp_dir().join(format!("savescummer-xtask-{}.exe", std::process::id()));
    let _ = fs::rename(&exe, aside);
}

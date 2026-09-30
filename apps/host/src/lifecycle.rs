//! Launch and normal quit requests for installed games.

use std::sync::Arc;

use savescummer_core::{ErrorKind, Failure};
use savescummer_platform::process;

use crate::host::Host;

pub fn play(host: &Arc<Host>, selector: &str) -> Result<bool, Failure> {
    let (id, steam_id, exe) = {
        let inner = host.lock();
        let id = host.find_game(&inner, selector)?;
        let game = inner.game(&id)?;
        if !game.installed {
            return Err(Failure::new(ErrorKind::NotFound, "game is no longer installed").game(&id));
        }
        if inner.stack.contains(&id) || inner.processes.contains_key(&id) {
            return Err(Failure::new(ErrorKind::Busy, "game is already running").game(&id));
        }
        let steam_id = game.steam_launch_id();
        let exe = game.main_executable();
        if steam_id.is_none() && exe.is_none() {
            return Err(Failure::new(ErrorKind::NotFound, "no game executable is known").game(&id));
        }
        (id, steam_id, exe)
    };
    if let Some(steam_id) = steam_id {
        process::launch_steam(steam_id).map_err(|error| {
            Failure::new(ErrorKind::Io, format!("couldn't ask Steam to launch the game: {error}")).game(&id)
        })?;
        return Ok(true);
    }
    let exe = exe.expect("checked above");
    if !exe.exists() {
        return Err(Failure::new(ErrorKind::NotFound, "game executable is missing").path(&exe));
    }
    process::launch_game(&exe)
        .map_err(|error| Failure::new(ErrorKind::Io, format!("couldn't launch game: {error}")).path(&exe))?;
    Ok(true)
}

pub fn close(host: &Arc<Host>, selector: &str) -> Result<bool, Failure> {
    let (id, pids) = {
        let inner = host.lock();
        let id = host.find_game(&inner, selector)?;
        let game = inner.game(&id)?;
        // Future Expert Mode will replace the wait_for_exit opt-out. For now,
        // that opt-out explicitly allows Close when the catalog has not vetted it.
        if !game.safe_to_close && game.wait_for_exit {
            return Err(Failure::new(ErrorKind::InvalidRequest, "closing this game is not enabled").game(&id));
        }
        let pids = inner.processes.get(&id).cloned().unwrap_or_default();
        if !inner.stack.contains(&id) || pids.is_empty() {
            return Err(Failure::new(ErrorKind::NotFound, "game is not running").game(&id));
        }
        (id, pids)
    };
    let mut sent = false;
    let mut last_error = None;
    for pid in pids {
        match process::request_game_close(pid) {
            Ok(()) => sent = true,
            Err(error) => last_error = Some(error),
        }
    }
    if sent {
        Ok(true)
    } else {
        Err(Failure::new(
            ErrorKind::Io,
            format!("couldn't ask the game to close: {}", last_error.expect("a running game has processes")),
        )
        .game(&id))
    }
}

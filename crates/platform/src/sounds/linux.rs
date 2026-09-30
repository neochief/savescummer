//! Linux: the WAV piped to the sound server's player, played to its end on
//! the sound worker. `pw-play` (PipeWire, the default on current distros
//! and SteamOS), else `paplay` (PulseAudio), else `aplay` (ALSA). No
//! temporary files. No player, no audio device, or any failure, is silence.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Each player with the arguments that make it read a WAV from stdin.
const PLAYERS: &[(&str, &[&str])] = &[("pw-play", &["-"]), ("paplay", &[]), ("aplay", &["-q", "-"])];

/// Longer than any cue: a player stuck on a dead device is stopped.
const LIMIT: Duration = Duration::from_secs(5);

pub fn play(wav: &'static [u8]) {
    let Some((program, args)) = player() else { return };
    let Ok(mut child) =
        Command::new(program).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(wav);
    }
    // Waits for the end, so the next cue never cuts this one off.
    let deadline = Instant::now() + LIMIT;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The first player on `PATH`, found once.
fn player() -> Option<&'static (&'static str, &'static [&'static str])> {
    static PLAYER: OnceLock<Option<&'static (&'static str, &'static [&'static str])>> = OnceLock::new();
    *PLAYER.get_or_init(|| {
        let path = std::env::var_os("PATH")?;
        PLAYERS.iter().find(|(program, _)| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    })
}

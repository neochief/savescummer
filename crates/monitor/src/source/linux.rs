//! The Linux process list from `/proc`: each process's parent, state and
//! start time from `/proc/<pid>/stat`, and its program from
//! `/proc/<pid>/exe`.
//!
//! A Windows game under Wine or Proton runs as Wine's preloader, so its
//! `exe` is Wine's. Wine puts the Windows program's path in the command
//! line instead (`Z:\home\me\...\Game.exe`); that path, turned back into a
//! Unix one through the prefix's drive links, is the process's program.
//! It's resolved like every path the monitor compares (PLAN-HOST.md,
//! MONITOR AND ACTIVE STACK): Wine's names are case-insensitive, so each
//! name is matched against the disk case-insensitively, then links are
//! resolved. Wine rewrites the command line only once the process is
//! running, so a Wine process first seen before that is looked at again.
//!
//! Another user's processes have no readable `exe`; like elevated
//! processes on Windows, they are listed without a path.
//!
//! The foreground process is the X11 window manager's `_NET_ACTIVE_WINDOW`
//! and that window's `_NET_WM_PID`. Wayland never tells other programs
//! which window is active, but in a Wayland session games run through
//! XWayland (all Wine and Proton ones, and most native ones), whose window
//! manager still says which X11 window is active. A native Wayland window in
//! front reads as no foreground process, as does a session without X.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

use super::{Proc, ProcessSource};

/// Caches each process's program and parent by pid and start time, so a
/// poll reads only `stat` for processes it has seen. The parent is the one
/// first seen: when a launcher exits, Linux hands its child to init or a
/// subreaper (`systemd --user`), and the child must still count as the
/// launcher's game.
#[derive(Default)]
pub struct LinuxSource {
    known: HashMap<(u32, u64), Known>,
    pids: HashSet<u32>,
    display: Display,
}

impl ProcessSource for LinuxSource {
    fn list(&mut self) -> Vec<Proc> {
        let mut fresh = HashMap::new();
        let mut procs = Vec::new();
        for pid in all_pids() {
            let Some(stat) = stat(pid) else { continue };
            // A zombie has exited; only its parent hasn't noticed yet.
            if stat.zombie {
                continue;
            }
            let key = (pid, stat.start);
            let known = match self.known.get(&key) {
                Some(known) if known.retries == 0 => known.clone(),
                // A Wine process whose Windows program wasn't readable yet.
                Some(known) => Known { parent: known.parent, ..Known::look(pid, known.retries - 1) },
                None => Known { parent: stat.parent, ..Known::look(pid, WINE_RETRIES) },
            };
            procs.push(Proc { pid, parent: known.parent, exe: known.exe.clone() });
            fresh.insert(key, known);
        }
        self.known = fresh;
        self.pids = procs.iter().map(|p| p.pid).collect();
        procs
    }

    fn may_have_changed(&mut self, watched: &[u32]) -> bool {
        if self.foreground().is_some_and(|pid| !self.pids.contains(&pid)) {
            return true;
        }
        // A game starting another program (a launcher starting the real
        // game): look now, while its parent is still known.
        for &pid in watched {
            let Some(children) = children(pid) else { return true };
            if children.iter().any(|child| !self.pids.contains(child)) {
                return true;
            }
        }
        watched.iter().any(|&pid| stat(pid).is_none_or(|stat| stat.zombie))
    }

    fn foreground(&mut self) -> Option<u32> {
        self.display.active_pid()
    }
}

/// The X display, connected on first use. A lost or missing one is asked
/// again at most every [`RECONNECT_EVERY`], not on every poll.
#[derive(Default)]
struct Display {
    x: Option<X>,
    last_try: Option<Instant>,
}

struct X {
    conn: RustConnection,
    root: Window,
    active_window: Atom,
    wm_pid: Atom,
}

const RECONNECT_EVERY: Duration = Duration::from_secs(10);

impl Display {
    fn active_pid(&mut self) -> Option<u32> {
        if self.x.is_none() && self.last_try.is_none_or(|last| last.elapsed() >= RECONNECT_EVERY) {
            self.last_try = Some(Instant::now());
            self.x = X::connect();
        }
        let x = self.x.as_ref()?;
        match x.active_pid() {
            Ok(pid) => pid,
            Err(()) => {
                self.x = None;
                None
            }
        }
    }
}

impl X {
    fn connect() -> Option<X> {
        std::env::var_os("DISPLAY")?;
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom);
        let active_window = atom(b"_NET_ACTIVE_WINDOW")?;
        let wm_pid = atom(b"_NET_WM_PID")?;
        Some(X { conn, root, active_window, wm_pid })
    }

    /// Err: the connection is gone.
    fn active_pid(&self) -> Result<Option<u32>, ()> {
        let Some(window) = self.property(self.root, self.active_window, AtomEnum::WINDOW)? else {
            return Ok(None);
        };
        if window == 0 {
            return Ok(None);
        }
        // A window that closed in between has no properties: no pid.
        Ok(self.property(window, self.wm_pid, AtomEnum::CARDINAL).unwrap_or(None).filter(|&pid| pid != 0))
    }

    fn property(&self, window: Window, name: Atom, kind: AtomEnum) -> Result<Option<u32>, ()> {
        let cookie = self.conn.get_property(false, window, name, kind, 0, 1).map_err(|_| ())?;
        match cookie.reply() {
            Ok(reply) => Ok(reply.value32().and_then(|mut values| values.next())),
            // An X error (the window is gone) isn't a lost connection.
            Err(x11rb::errors::ReplyError::X11Error(_)) => Ok(None),
            Err(_) => Err(()),
        }
    }
}

fn all_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries.filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok()).filter(|&pid| pid != 0).collect()
}

struct Stat {
    parent: u32,
    zombie: bool,
    /// Clock ticks after boot; with the pid, names one process.
    start: u64,
}

/// A process's parent, state and start time; None once it's gone.
fn stat(pid: u32) -> Option<Stat> {
    parse_stat(&std::fs::read(format!("/proc/{pid}/stat")).ok()?)
}

/// `pid (comm) state ppid ... starttime ...`: the name may hold spaces and
/// parentheses, so the fields are counted from its last `)`.
fn parse_stat(text: &[u8]) -> Option<Stat> {
    let end = text.iter().rposition(|&b| b == b')')?;
    let rest = std::str::from_utf8(&text[end + 1..]).ok()?;
    let fields: Vec<&str> = rest.split_ascii_whitespace().collect();
    // Fields 3 (state), 4 (ppid) and 22 (starttime) of proc(5).
    Some(Stat {
        zombie: matches!(*fields.first()?, "Z" | "X"),
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
    })
}

/// The processes `pid` started that are still its children; None when the
/// kernel doesn't say (no `CONFIG_PROC_CHILDREN`, or the process is gone).
fn children(pid: u32) -> Option<Vec<u32>> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut children = Vec::new();
    for task in tasks {
        let text = std::fs::read_to_string(task.ok()?.path().join("children")).ok()?;
        children.extend(text.split_ascii_whitespace().filter_map(|p| p.parse::<u32>().ok()));
    }
    Some(children)
}

/// What's remembered about one process.
#[derive(Clone)]
struct Known {
    parent: u32,
    exe: Option<PathBuf>,
    /// How many more full looks may ask again for a Wine process's Windows
    /// program; 0 once it's known (or the process isn't Wine's).
    retries: u8,
}

/// Full looks that ask again for a Wine process's Windows program: Wine
/// sets it within moments of starting, and a Wine process that never does
/// (a helper that isn't a Windows program) stops costing reads.
const WINE_RETRIES: u8 = 8;

impl Known {
    /// The program a process runs; None for another user's process. A Wine
    /// process without a readable Windows program yet is Wine's loader for
    /// now, with `retries` more looks.
    fn look(pid: u32, retries: u8) -> Known {
        let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
            return Known { parent: 0, exe: None, retries: 0 };
        };
        let exe = without_deleted(exe);
        if !is_wine(&exe) {
            return Known { parent: 0, exe: Some(exe), retries: 0 };
        }
        match windows_program(pid) {
            Some(windows) => Known { parent: 0, exe: Some(real_file(&windows).unwrap_or(windows)), retries: 0 },
            None => Known { parent: 0, exe: Some(exe), retries },
        }
    }
}

/// A path as it is on disk: each name matched case-insensitively when the
/// exact one isn't there (Wine's names ignore case; Linux's don't), then
/// links resolved. None when a name is missing, or matches several names
/// that differ only in case: Wine itself can't tell those apart reliably.
fn real_file(path: &Path) -> Option<PathBuf> {
    let mut found = PathBuf::new();
    for part in path.components() {
        let exact = found.join(part);
        if exact.symlink_metadata().is_ok() || !matches!(part, std::path::Component::Normal(_)) {
            found = exact;
            continue;
        }
        let wanted = part.as_os_str().to_str()?.to_lowercase();
        let mut matches = std::fs::read_dir(&found)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .filter(|name| name.to_str().is_some_and(|name| name.to_lowercase() == wanted));
        let name = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        found.push(name);
    }
    std::fs::canonicalize(found).ok()
}

/// A replaced program (a game updated while it runs) reads as
/// `/path (deleted)`.
fn without_deleted(exe: PathBuf) -> PathBuf {
    let bytes = exe.as_os_str().as_bytes();
    match bytes.strip_suffix(b" (deleted)") {
        Some(path) => PathBuf::from(OsString::from_vec(path.to_vec())),
        None => exe,
    }
}

/// Wine's loaders, which run every Windows program: `wine64-preloader`,
/// `wine-preloader`, `wine64` and `wine`, in Wine or in a Proton build.
fn is_wine(exe: &Path) -> bool {
    exe.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "wine" | "wine64" | "wine-preloader" | "wine64-preloader"))
}

/// The Windows program a Wine process runs, as a Unix path.
fn windows_program(pid: u32) -> Option<PathBuf> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let first = cmdline.split(|&b| b == 0).next()?;
    let windows = std::str::from_utf8(first).ok()?;
    let prefix = wine_prefix(pid);
    unix_path(windows, |drive| drive_target(prefix.as_deref(), drive))
}

/// `WINEPREFIX` from the process's environment, else Wine's default.
fn wine_prefix(pid: u32) -> Option<PathBuf> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let from_env = environ
        .split(|&b| b == 0)
        .find_map(|entry| entry.strip_prefix(b"WINEPREFIX="))
        .map(|value| PathBuf::from(OsString::from_vec(value.to_vec())));
    from_env.or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".wine")))
}

/// Where a drive letter points in a Wine prefix: its `dosdevices/<x>:` link.
fn drive_target(prefix: Option<&Path>, drive: char) -> Option<PathBuf> {
    let link = prefix?.join("dosdevices").join(format!("{drive}:"));
    std::fs::canonicalize(link).ok()
}

/// `X:\dir\Game.exe` as a Unix path, with `drive` saying where a drive
/// letter points. `Z:` is `/` in every Wine prefix, so it needs no lookup.
fn unix_path(windows: &str, drive: impl FnOnce(char) -> Option<PathBuf>) -> Option<PathBuf> {
    let mut chars = windows.chars();
    let letter = chars.next()?.to_ascii_lowercase();
    if !letter.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let rest = chars.as_str().trim_start_matches(['\\', '/']);
    if rest.is_empty() {
        return None;
    }
    let root = if letter == 'z' { PathBuf::from("/") } else { drive(letter)? };
    Some(rest.split(['\\', '/']).filter(|part| !part.is_empty()).fold(root, |path, part| path.join(part)))
}

pub fn system_source() -> Box<dyn ProcessSource> {
    Box::new(LinuxSource::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_fields_count_from_the_end_of_the_name() {
        let line = b"4242 (Game (x86) ) S 17 4242 4242 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 987654 0 0";
        let stat = parse_stat(line).unwrap();
        assert_eq!(stat.parent, 17);
        assert_eq!(stat.start, 987654);
        assert!(!stat.zombie);
        let zombie = parse_stat(b"7 (x) Z 1 7 7 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5 0 0").unwrap();
        assert!(zombie.zombie);
        assert!(parse_stat(b"7 (x) S").is_none());
    }

    #[test]
    fn wine_paths_become_unix_paths() {
        let none = |_| None;
        assert_eq!(
            unix_path(r"Z:\home\deck\Games\Hades\Hades.exe", none),
            Some(PathBuf::from("/home/deck/Games/Hades/Hades.exe"))
        );
        assert_eq!(
            unix_path(r"C:\Program Files\Game\game.exe", |d| (d == 'c').then(|| PathBuf::from("/pfx/drive_c"))),
            Some(PathBuf::from("/pfx/drive_c/Program Files/Game/game.exe"))
        );
        assert_eq!(unix_path(r"C:\game.exe", none), None, "an unknown drive isn't guessed");
        assert_eq!(unix_path("/usr/bin/wine64", none), None);
        assert_eq!(unix_path("Z:", none), None);
    }

    #[test]
    fn a_replaced_program_keeps_its_path() {
        assert_eq!(without_deleted(PathBuf::from("/g/game (deleted)")), PathBuf::from("/g/game"));
        assert_eq!(without_deleted(PathBuf::from("/g/game")), PathBuf::from("/g/game"));
    }

    #[test]
    fn wine_loaders_are_recognised() {
        assert!(is_wine(Path::new("/proton/files/bin/wine64-preloader")));
        assert!(is_wine(Path::new("/usr/bin/wine")));
        assert!(!is_wine(Path::new("/usr/bin/winecfg")));
    }

    #[test]
    fn wine_names_match_the_disk_whatever_their_case() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("Steam Library/common/Game")).unwrap();
        std::fs::write(root.join("Steam Library/common/Game/Game.exe"), "").unwrap();
        let expected = root.join("Steam Library/common/Game/Game.exe");
        assert_eq!(real_file(&root.join("Steam Library/common/Game/Game.exe")), Some(expected.clone()));
        assert_eq!(real_file(&root.join("steam library/COMMON/game/game.EXE")), Some(expected.clone()));

        // Through a link, as a Steam library often is: the real path.
        std::os::unix::fs::symlink(root.join("Steam Library"), root.join("link")).unwrap();
        assert_eq!(real_file(&root.join("link/common/game/game.exe")), Some(expected));

        // Missing, or ambiguous: not guessed.
        assert_eq!(real_file(&root.join("Steam Library/common/Game/Other.exe")), None);
        std::fs::write(root.join("Steam Library/common/Game/GAME.EXE"), "").unwrap();
        assert_eq!(real_file(&root.join("Steam Library/common/Game/game.exe")), None);
    }

    #[test]
    fn this_process_is_listed_with_its_program() {
        let mut source = LinuxSource::default();
        let me = std::process::id();
        let procs = source.list();
        let this = procs.iter().find(|p| p.pid == me).expect("this process is listed");
        assert_eq!(this.exe.as_deref(), Some(std::env::current_exe().unwrap().as_path()));
        assert_eq!(this.parent, std::os::unix::process::parent_id());
        assert!(!source.may_have_changed(&[me]) || children(me).is_none());
    }
}

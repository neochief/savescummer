//! Watches games' processes start, get focus and exit.
//!
//! Processes are matched to games by their full executable path, so an
//! unrelated program with the same file name elsewhere doesn't count. Two
//! additions make real games work: a process started by a game's process
//! belongs to that game (launchers that start the game and exit), and for a
//! known game any program inside its install folder is the game (Slay the
//! Spire runs as `jre/bin/javaw.exe` under its install folder).
//!
//! The monitor polls: a process list is cheap, it sees processes that
//! started before the monitor, and it needs no elevated rights.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// One running process as the OS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Full executable path; None when the OS won't say (an elevated or
    /// protected process).
    pub exe: Option<PathBuf>,
}

pub trait ProcessSource: Send {
    fn list(&mut self) -> Vec<Proc>;
    /// The process owning the foreground window.
    fn foreground(&mut self) -> Option<u32>;
}

/// What identifies one game's processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProcesses {
    pub game: String,
    pub executables: Vec<PathBuf>,
    /// For known games: any program inside it counts.
    pub install_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `observed` is false for games already running when the monitor
    /// started: their start time is unknown and must not be invented.
    Started {
        game: String,
        observed: bool,
    },
    Exited {
        game: String,
    },
    Focused {
        game: String,
    },
}

pub struct Monitor {
    source: Box<dyn ProcessSource>,
    exact: HashMap<String, String>,
    dirs: Vec<(String, String)>,
    /// pid → (parent, game) of processes seen on the last poll.
    seen: HashMap<u32, (u32, Option<String>)>,
    running: BTreeMap<String, BTreeSet<u32>>,
    focused: Option<String>,
    first_poll: bool,
}

pub fn normalize(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let text = text.replace('\\', "/");
    if cfg!(windows) || cfg!(target_os = "macos") { text.to_lowercase() } else { text }
}

impl Monitor {
    pub fn new(source: Box<dyn ProcessSource>) -> Monitor {
        Monitor {
            source,
            exact: HashMap::new(),
            dirs: Vec::new(),
            seen: HashMap::new(),
            running: BTreeMap::new(),
            focused: None,
            first_poll: true,
        }
    }

    /// Replaces which executables belong to which game. Processes are
    /// matched again on the next poll.
    pub fn set_games(&mut self, games: &[GameProcesses]) {
        self.exact.clear();
        self.dirs.clear();
        for game in games {
            for exe in &game.executables {
                self.exact.insert(normalize(exe), game.game.clone());
            }
            if let Some(dir) = &game.install_dir {
                let mut key = normalize(dir);
                if !key.ends_with('/') {
                    key.push('/');
                }
                self.dirs.push((key, game.game.clone()));
            }
        }
        // Longest install folder first, so nested installs match precisely.
        self.dirs.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        // Processes already assigned keep their game: a game's child whose
        // launcher has exited can't be traced back again.
        let known: BTreeSet<&str> = games.iter().map(|g| g.game.as_str()).collect();
        for entry in self.seen.values_mut() {
            if entry.1.as_deref().is_some_and(|g| !known.contains(g)) {
                entry.1 = None;
            }
        }
    }

    pub fn running(&self) -> Vec<String> {
        self.running.keys().cloned().collect()
    }

    pub fn is_running(&self, game: &str) -> bool {
        self.running.contains_key(game)
    }

    fn match_exe(&self, exe: &Path) -> Option<String> {
        let key = normalize(exe);
        if let Some(game) = self.exact.get(&key) {
            return Some(game.clone());
        }
        self.dirs.iter().find(|(dir, _)| key.starts_with(dir.as_str())).map(|(_, g)| g.clone())
    }

    /// Takes one look at the process list and reports what changed.
    pub fn poll(&mut self) -> Vec<Event> {
        let processes = self.source.list();
        let mut next: HashMap<u32, (u32, Option<String>)> = HashMap::new();
        // Parents before children where possible: process lists are mostly
        // in creation order, and a second pass catches the rest.
        for pass in 0..2 {
            for proc in &processes {
                if pass == 1 && next.get(&proc.pid).is_some_and(|e| e.1.is_some()) {
                    continue;
                }
                let game = match self.seen.get(&proc.pid) {
                    // The same process as before (same parent): keep its game.
                    Some((parent, game)) if *parent == proc.parent && game.is_some() => game.clone(),
                    _ => proc.exe.as_deref().and_then(|exe| self.match_exe(exe)).or_else(|| {
                        next.get(&proc.parent).or_else(|| self.seen.get(&proc.parent)).and_then(|(_, g)| g.clone())
                    }),
                };
                next.insert(proc.pid, (proc.parent, game));
            }
        }

        let mut running: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for (pid, (_, game)) in &next {
            if let Some(game) = game {
                running.entry(game.clone()).or_default().insert(*pid);
            }
        }

        let mut events = Vec::new();
        for game in running.keys() {
            if !self.running.contains_key(game) {
                events.push(Event::Started { game: game.clone(), observed: !self.first_poll });
            }
        }
        for game in self.running.keys() {
            if !running.contains_key(game) {
                events.push(Event::Exited { game: game.clone() });
                if self.focused.as_deref() == Some(game) {
                    self.focused = None;
                }
            }
        }

        let foreground = self.source.foreground();
        let focused_game = foreground.and_then(|pid| next.get(&pid)).and_then(|(_, g)| g.clone());
        if let Some(game) = focused_game
            && self.focused.as_deref() != Some(game.as_str())
        {
            events.push(Event::Focused { game: game.clone() });
            self.focused = Some(game);
        } else if foreground.is_some()
            && self.focused.is_some()
            && next.get(&foreground.unwrap()).is_none_or(|(_, g)| g.is_none())
        {
            // Another app is in front; the next switch back is a new focus.
            self.focused = None;
        }

        self.seen = next;
        self.running = running;
        self.first_poll = false;
        events
    }
}

/// The OS process list.
pub fn system_source() -> Box<dyn ProcessSource> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsSource::default())
    }
    #[cfg(not(windows))]
    {
        Box::new(Unsupported)
    }
}

#[cfg(not(windows))]
struct Unsupported;

#[cfg(not(windows))]
impl ProcessSource for Unsupported {
    fn list(&mut self) -> Vec<Proc> {
        Vec::new()
    }
    fn foreground(&mut self) -> Option<u32> {
        None
    }
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    use super::{Proc, ProcessSource};

    /// Caches each process's path by pid and parent, so a poll opens only
    /// processes it hasn't seen.
    #[derive(Default)]
    pub struct WindowsSource {
        paths: HashMap<(u32, u32), Option<PathBuf>>,
    }

    fn image_path(pid: u32) -> Option<PathBuf> {
        // SAFETY: plain Win32 calls with a buffer we own; the handle is closed.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; 32768];
            let mut len = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize])))
        }
    }

    impl ProcessSource for WindowsSource {
        fn list(&mut self) -> Vec<Proc> {
            let mut out: Vec<(u32, u32)> = Vec::new();
            // SAFETY: the snapshot handle is closed below; the entry struct is
            // initialized with its size as the API requires.
            unsafe {
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
                if snapshot == INVALID_HANDLE_VALUE {
                    return Vec::new();
                }
                let mut entry: PROCESSENTRY32W = std::mem::zeroed();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                let mut ok = Process32FirstW(snapshot, &mut entry);
                while ok != 0 {
                    out.push((entry.th32ProcessID, entry.th32ParentProcessID));
                    ok = Process32NextW(snapshot, &mut entry);
                }
                CloseHandle(snapshot);
            }
            let mut fresh = HashMap::new();
            let procs = out
                .into_iter()
                .filter(|(pid, _)| *pid != 0 && *pid != 4)
                .map(|(pid, parent)| {
                    let exe = match self.paths.get(&(pid, parent)) {
                        Some(path) => path.clone(),
                        None => image_path(pid),
                    };
                    fresh.insert((pid, parent), exe.clone());
                    Proc { pid, parent, exe }
                })
                .collect();
            self.paths = fresh;
            procs
        }

        fn foreground(&mut self) -> Option<u32> {
            // SAFETY: reading the foreground window's owner has no preconditions.
            unsafe {
                let window = GetForegroundWindow();
                if window.is_null() {
                    return None;
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(window, &mut pid);
                (pid != 0).then_some(pid)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<(Vec<Proc>, Option<u32>)>>);

    impl Fake {
        fn set(&self, procs: &[(u32, u32, &str)], foreground: Option<u32>) {
            let list = procs
                .iter()
                .map(|(pid, parent, exe)| Proc {
                    pid: *pid,
                    parent: *parent,
                    exe: if exe.is_empty() { None } else { Some(PathBuf::from(exe)) },
                })
                .collect();
            *self.0.lock().unwrap() = (list, foreground);
        }
    }

    impl ProcessSource for Fake {
        fn list(&mut self) -> Vec<Proc> {
            self.0.lock().unwrap().0.clone()
        }
        fn foreground(&mut self) -> Option<u32> {
            self.0.lock().unwrap().1
        }
    }

    fn monitor() -> (Monitor, Fake) {
        let fake = Fake::default();
        let mut m = Monitor::new(Box::new(fake.clone()));
        m.set_games(&[
            GameProcesses {
                game: "ftl".into(),
                executables: vec!["C:/Games/FTL/FTLGame.exe".into()],
                install_dir: None,
            },
            GameProcesses {
                game: "sts".into(),
                executables: vec!["D:/Steam/common/SlayTheSpire/SlayTheSpire.exe".into()],
                install_dir: Some("D:/Steam/common/SlayTheSpire".into()),
            },
        ]);
        (m, fake)
    }

    fn started(game: &str, observed: bool) -> Event {
        Event::Started { game: game.into(), observed }
    }

    #[test]
    fn games_running_before_the_monitor_are_not_observed_starts() {
        let (mut m, fake) = monitor();
        fake.set(&[(10, 1, "C:/Games/FTL/FTLGame.exe")], None);
        assert_eq!(m.poll(), vec![started("ftl", false)]);
        fake.set(
            &[(10, 1, "C:/Games/FTL/FTLGame.exe"), (20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe")],
            None,
        );
        assert_eq!(m.poll(), vec![started("sts", true)]);
    }

    #[test]
    fn several_processes_are_one_game_until_the_last_exits() {
        let (mut m, fake) = monitor();
        fake.set(&[], None);
        m.poll();
        fake.set(&[(10, 1, "C:/Games/FTL/FTLGame.exe"), (11, 1, "c:\\games\\ftl\\ftlgame.exe")], None);
        assert_eq!(m.poll(), vec![started("ftl", true)]);
        fake.set(&[(11, 1, "C:/Games/FTL/FTLGame.exe")], None);
        assert!(m.poll().is_empty());
        fake.set(&[], None);
        assert_eq!(m.poll(), vec![Event::Exited { game: "ftl".into() }]);
    }

    #[test]
    fn same_file_name_elsewhere_doesnt_count() {
        let (mut m, fake) = monitor();
        fake.set(&[(10, 1, "C:/Other/FTLGame.exe")], None);
        assert!(m.poll().is_empty());
    }

    #[test]
    fn a_launcher_that_starts_the_game_and_exits() {
        let (mut m, fake) = monitor();
        fake.set(&[], None);
        m.poll();
        fake.set(&[(20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe")], None);
        assert_eq!(m.poll(), vec![started("sts", true)]);
        // The launcher starts a child elsewhere and exits.
        fake.set(&[(21, 20, "C:/Java/bin/javaw.exe")], None);
        assert!(m.poll().is_empty(), "the child keeps the game running");
        fake.set(&[], None);
        assert_eq!(m.poll(), vec![Event::Exited { game: "sts".into() }]);
    }

    #[test]
    fn programs_inside_a_known_install_folder_are_the_game() {
        let (mut m, fake) = monitor();
        fake.set(&[], None);
        m.poll();
        fake.set(&[(30, 1, "D:/Steam/common/SlayTheSpire/jre/bin/javaw.exe")], None);
        assert_eq!(m.poll(), vec![started("sts", true)]);
    }

    #[test]
    fn focus_follows_the_foreground_window() {
        let (mut m, fake) = monitor();
        fake.set(
            &[(10, 1, "C:/Games/FTL/FTLGame.exe"), (20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe")],
            Some(10),
        );
        let events = m.poll();
        assert!(events.contains(&Event::Focused { game: "ftl".into() }));
        assert!(m.poll().is_empty(), "no repeated focus events");
        fake.set(
            &[(10, 1, "C:/Games/FTL/FTLGame.exe"), (20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe")],
            Some(20),
        );
        assert_eq!(m.poll(), vec![Event::Focused { game: "sts".into() }]);
        // An unrelated app in front, then back to STS: a new focus.
        fake.set(
            &[
                (10, 1, "C:/Games/FTL/FTLGame.exe"),
                (20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe"),
                (99, 1, "C:/x.exe"),
            ],
            Some(99),
        );
        assert!(m.poll().is_empty());
        fake.set(
            &[(10, 1, "C:/Games/FTL/FTLGame.exe"), (20, 1, "D:/Steam/common/SlayTheSpire/SlayTheSpire.exe")],
            Some(20),
        );
        assert_eq!(m.poll(), vec![Event::Focused { game: "sts".into() }]);
    }

    #[test]
    fn quick_relaunch_between_polls_is_one_session() {
        let (mut m, fake) = monitor();
        fake.set(&[(10, 1, "C:/Games/FTL/FTLGame.exe")], None);
        m.poll();
        fake.set(&[(12, 1, "C:/Games/FTL/FTLGame.exe")], None);
        assert!(m.poll().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_real_process_list_includes_this_test() {
        let mut source = system_source();
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        let list = source.list();
        let mine = list.iter().find(|p| p.pid == me).expect("this process is listed");
        assert_eq!(normalize(mine.exe.as_ref().unwrap()), normalize(&exe));
    }
}

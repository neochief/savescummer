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
    /// Whether a full look may find something new since the last `list`:
    /// one of `watched` (the running games' processes) exited, or an
    /// unknown process is in front. A source that can't tell cheaply says
    /// yes. The monitor also takes a full look every [`FULL_EVERY`] polls,
    /// for programs that start in the background.
    fn may_have_changed(&mut self, watched: &[u32]) -> bool {
        let _ = watched;
        true
    }
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

/// A full look at the process list at least every this many polls.
/// Why not every poll: listing processes is a kernel query over every
/// process and thread on the machine, and doing it four times a second was
/// nearly all of an idle host's CPU time. Exits of running games and games
/// taking the foreground are still seen at the next poll.
pub const FULL_EVERY: u32 = 8;

pub struct Monitor {
    source: Box<dyn ProcessSource>,
    exact: HashMap<String, String>,
    dirs: Vec<(String, String)>,
    /// pid → (parent, game) of processes seen on the last poll.
    seen: HashMap<u32, (u32, Option<String>)>,
    running: BTreeMap<String, BTreeSet<u32>>,
    focused: Option<String>,
    first_poll: bool,
    /// The games changed since the last full look.
    games_changed: bool,
    /// Polls since the last full look.
    quiet_polls: u32,
}

/// The path with links resolved, as the OS reports a process's executable
/// (`/private/var/...` for `/var/...` on macOS). Windows paths are compared
/// as given.
fn real_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        return path.to_path_buf();
    }
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
            games_changed: true,
            quiet_polls: 0,
        }
    }

    /// Replaces which executables belong to which game. Processes are
    /// matched again on the next poll.
    pub fn set_games(&mut self, games: &[GameProcesses]) {
        self.exact.clear();
        self.dirs.clear();
        for game in games {
            for exe in &game.executables {
                let key = normalize(&real_path(exe));
                // A macOS app bundle runs as `Foo.app/Contents/MacOS/Foo`:
                // anything inside the listed bundle is that executable.
                if key.ends_with(".app") {
                    self.dirs.push((format!("{key}/"), game.game.clone()));
                }
                self.exact.insert(key, game.game.clone());
            }
            if let Some(dir) = &game.install_dir {
                let mut key = normalize(&real_path(dir));
                if !key.ends_with('/') {
                    key.push('/');
                }
                self.dirs.push((key, game.game.clone()));
            }
        }
        self.games_changed = true;
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

    /// The game in front, as of the last poll.
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// Every running game's processes.
    pub fn processes(&self) -> BTreeMap<String, Vec<u32>> {
        self.running.iter().map(|(game, pids)| (game.clone(), pids.iter().copied().collect())).collect()
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
        // Nothing suggests a change and a full look isn't due: only focus
        // is checked.
        let watched: Vec<u32> = self.running.values().flatten().copied().collect();
        self.quiet_polls += 1;
        if !self.first_poll
            && !self.games_changed
            && self.quiet_polls < FULL_EVERY
            && !self.source.may_have_changed(&watched)
        {
            let mut events = Vec::new();
            let seen = std::mem::take(&mut self.seen);
            self.check_focus(&seen, &mut events);
            self.seen = seen;
            return events;
        }
        self.games_changed = false;
        self.quiet_polls = 0;
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

        self.check_focus(&next, &mut events);

        self.seen = next;
        self.running = running;
        self.first_poll = false;
        events
    }

    /// Reports a game coming to the front; forgets focus when another app is.
    fn check_focus(&mut self, seen: &HashMap<u32, (u32, Option<String>)>, events: &mut Vec<Event>) {
        let foreground = self.source.foreground();
        let focused_game = foreground.and_then(|pid| seen.get(&pid)).and_then(|(_, g)| g.clone());
        if let Some(game) = focused_game
            && self.focused.as_deref() != Some(game.as_str())
        {
            events.push(Event::Focused { game: game.clone() });
            self.focused = Some(game);
        } else if foreground.is_some()
            && self.focused.is_some()
            && seen.get(&foreground.unwrap()).is_none_or(|(_, g)| g.is_none())
        {
            // Another app is in front; the next switch back is a new focus.
            self.focused = None;
        }
    }
}

pub mod open_files;

// The OS process list, one file per OS.
#[cfg_attr(windows, path = "source/windows.rs")]
#[cfg_attr(target_os = "macos", path = "source/macos.rs")]
#[cfg_attr(not(any(windows, target_os = "macos")), path = "source/unsupported.rs")]
mod source;

/// The OS process list.
pub fn system_source() -> Box<dyn ProcessSource> {
    source::system_source()
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
    fn a_listed_app_bundle_matches_its_executable_inside() {
        let fake = Fake::default();
        let mut m = Monitor::new(Box::new(fake.clone()));
        m.set_games(&[GameProcesses {
            game: "foo".into(),
            executables: vec!["/Applications/Foo.app".into()],
            install_dir: None,
        }]);
        fake.set(
            &[(10, 1, "/Applications/Foo.app/Contents/MacOS/Foo"), (11, 1, "/Applications/Foo.application")],
            None,
        );
        assert_eq!(m.poll(), vec![started("foo", false)]);
        assert_eq!(m.processes()["foo"], vec![10]);
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

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn the_real_process_list_includes_this_test() {
        let mut source = system_source();
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        let list = source.list();
        let mine = list.iter().find(|p| p.pid == me).expect("this process is listed");
        assert_eq!(normalize(mine.exe.as_ref().unwrap()), normalize(&exe));
    }

    /// Processes, the foreground pid, "may have changed", and list calls.
    type QuietState = (Vec<Proc>, Option<u32>, bool, usize);

    /// A source that reports "unchanged" until told otherwise, counting how
    /// often it is listed.
    #[derive(Clone, Default)]
    struct Quiet(Arc<Mutex<QuietState>>);

    impl ProcessSource for Quiet {
        fn list(&mut self) -> Vec<Proc> {
            let mut s = self.0.lock().unwrap();
            s.2 = false;
            s.3 += 1;
            s.0.clone()
        }
        fn may_have_changed(&mut self, _watched: &[u32]) -> bool {
            self.0.lock().unwrap().2
        }
        fn foreground(&mut self) -> Option<u32> {
            self.0.lock().unwrap().1
        }
    }

    #[test]
    fn an_unchanged_process_list_is_not_matched_again_but_focus_still_counts() {
        let quiet = Quiet::default();
        let game = Proc { pid: 10, parent: 1, exe: Some(PathBuf::from(r"C:GamesA.exe")) };
        *quiet.0.lock().unwrap() = (vec![game], None, true, 0);
        let mut m = Monitor::new(Box::new(quiet.clone()));
        let games =
            [GameProcesses { game: "a".into(), executables: vec![PathBuf::from(r"C:GamesA.exe")], install_dir: None }];
        m.set_games(&games);
        assert_eq!(m.poll(), vec![Event::Started { game: "a".into(), observed: false }]);
        assert!(m.poll().is_empty());
        assert_eq!(quiet.0.lock().unwrap().3, 1, "listed once");

        // Focus moves without any process starting.
        quiet.0.lock().unwrap().1 = Some(10);
        assert_eq!(m.poll(), vec![Event::Focused { game: "a".into() }]);
        assert_eq!(quiet.0.lock().unwrap().3, 1);

        // New games: matched again even though no process changed.
        m.set_games(&games);
        m.poll();
        assert_eq!(quiet.0.lock().unwrap().3, 2);

        // A process exits: the source says so, and the exit is seen.
        let mut s = quiet.0.lock().unwrap();
        s.0.clear();
        s.2 = true;
        drop(s);
        assert_eq!(m.poll(), vec![Event::Exited { game: "a".into() }]);
    }
}

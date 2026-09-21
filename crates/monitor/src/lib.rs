#[cfg(test)]
use savescummer_core::GameOrigin;
use savescummer_core::{Game, Id, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub executable: PathBuf,
}
pub struct Observation {
    pub processes: Vec<Process>,
    pub foreground_pid: Option<u32>,
}
pub trait ObservationSource {
    fn observe(&self) -> Result<Observation>;
}
#[derive(Debug, Default)]
pub struct Activity {
    pub stack: Vec<Id>,
    pub started: Vec<Id>,
    pub closed: Vec<Id>,
}
#[derive(Debug, Default)]
pub struct Monitor {
    running: BTreeSet<Id>,
    stack: Vec<Id>,
    initialized: bool,
}
fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
impl Monitor {
    pub fn observe(&mut self, games: &BTreeMap<Id, Game>, observation: Observation) -> Activity {
        let mut running = BTreeSet::new();
        let mut foreground = None;
        for process in observation.processes {
            for game in games.values() {
                if game
                    .executables
                    .iter()
                    .any(|path| same_path(path, &process.executable))
                {
                    running.insert(game.id.clone());
                    if Some(process.pid) == observation.foreground_pid {
                        foreground = Some(game.id.clone());
                    }
                }
            }
        }
        let started = if self.initialized {
            running.difference(&self.running).cloned().collect()
        } else {
            vec![]
        };
        let closed = self.running.difference(&running).cloned().collect();
        self.stack.retain(|id| running.contains(id));
        // Deterministic stable-ID order for simultaneous/initial observations.
        for id in running
            .difference(&self.running)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            self.stack.insert(0, id.clone());
        }
        if let Some(id) = foreground {
            self.stack.retain(|g| g != &id);
            self.stack.insert(0, id);
        }
        self.running = running;
        self.initialized = true;
        Activity {
            stack: self.stack.clone(),
            started,
            closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game(id: &str) -> Game {
        Game {
            detected_locations: vec![],
            user_configured: true,
            id: id.into(),
            name: id.into(),
            origin: GameOrigin::Known,
            info: String::new(),
            data_dir: PathBuf::from(id),
            executables: vec![PathBuf::from(format!("/{id}/game.exe"))],
            installed: true,
            configuration_error: None,
        }
    }
    fn obs(processes: &[(u32, &str)], focus: Option<u32>) -> Observation {
        Observation {
            processes: processes
                .iter()
                .map(|(pid, path)| Process {
                    pid: *pid,
                    executable: path.into(),
                })
                .collect(),
            foreground_pid: focus,
        }
    }
    #[test]
    fn stack_tracks_games_not_windows_or_individual_processes() {
        let games = [game("a"), game("b")]
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();
        let mut monitor = Monitor::default();
        assert!(monitor.observe(&games, obs(&[], None)).stack.is_empty());
        let activity = monitor.observe(
            &games,
            obs(
                &[(1, "/a/game.exe"), (2, "/a/game.exe"), (3, "/b/game.exe")],
                Some(3),
            ),
        );
        assert_eq!(activity.started, ["a", "b"]);
        assert_eq!(activity.stack, ["b", "a"]);
        let activity = monitor.observe(
            &games,
            obs(&[(1, "/a/game.exe"), (3, "/b/game.exe")], Some(1)),
        );
        assert!(activity.closed.is_empty());
        assert_eq!(activity.stack, ["a", "b"]);
        let activity = monitor.observe(
            &games,
            obs(&[(3, "/b/game.exe"), (4, "/unrelated/game.exe")], Some(4)),
        );
        assert_eq!(activity.closed, ["a"]);
        assert_eq!(activity.stack, ["b"]);
    }
    #[test]
    fn startup_does_not_invent_launch_events() {
        let games = [game("a")].into_iter().map(|g| (g.id.clone(), g)).collect();
        let activity = Monitor::default().observe(&games, obs(&[(1, "/a/game.exe")], None));
        assert_eq!(activity.stack, ["a"]);
        assert!(activity.started.is_empty());
    }
}

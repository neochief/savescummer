//! The ACTIVE STACK: running games, ordered by which one the user switched
//! to last. And the active game, the hotkeys' target: the game the user was
//! last in, even after it closed.

use std::time::{Duration, Instant};

/// How long after the active game closes before a game in front takes over.
/// When a game quits, macOS and Windows bring the next window forward on
/// their own (after a fullscreen game's slide out of its Space), and that
/// isn't the user switching. A game still in front after this is the one
/// being played.
pub const SETTLE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveStack {
    /// Top first.
    entries: Vec<String>,
    /// The active game after it closed, and when it closed.
    closed_active: Option<(String, Instant)>,
}

impl ActiveStack {
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn top(&self) -> Option<&str> {
        self.entries.first().map(String::as_str)
    }

    /// The top, or the game that was on top when it closed, until another
    /// game takes over (`settle`).
    pub fn active(&self) -> Option<&str> {
        self.closed_active.as_ref().map(|(game, _)| game.as_str()).or_else(|| self.top())
    }

    pub fn contains(&self, game: &str) -> bool {
        self.entries.iter().any(|g| g == game)
    }

    /// A game that starts appears in the stack but doesn't jump ahead of
    /// games focused more recently. The closed active game is the one focused
    /// most recently, so it goes back on top.
    pub fn started(&mut self, game: &str) -> bool {
        if self.contains(game) {
            return false;
        }
        if self.closed_active.as_ref().is_some_and(|(closed, _)| closed == game) {
            self.closed_active = None;
            self.entries.insert(0, game.to_string());
        } else {
            self.entries.push(game.to_string());
        }
        true
    }

    /// A game switched to moves to the top. It's the active game once the
    /// closed active game settles (`settle`).
    pub fn focused(&mut self, game: &str) -> bool {
        match self.entries.iter().position(|g| g == game) {
            Some(0) => false,
            Some(i) => {
                let entry = self.entries.remove(i);
                self.entries.insert(0, entry);
                true
            }
            None => {
                self.entries.insert(0, game.to_string());
                true
            }
        }
    }

    /// When the last process of a game exits, it leaves the stack. If it was
    /// the active game, it stays active.
    pub fn exited(&mut self, game: &str, now: Instant) -> bool {
        if self.active() == Some(game) {
            self.closed_active = Some((game.to_string(), now));
        }
        let before = self.entries.len();
        self.entries.retain(|g| g != game);
        before != self.entries.len()
    }

    /// Called with the game in front at every look: [`SETTLE`] after the
    /// active game closed, a running game in front takes over from it.
    /// Returns whether the active game changed.
    pub fn settle(&mut self, front: Option<&str>, now: Instant) -> bool {
        let Some((_, closed_at)) = &self.closed_active else { return false };
        let Some(front) = front.filter(|g| self.contains(g)) else { return false };
        if now.saturating_duration_since(*closed_at) < SETTLE {
            return false;
        }
        let front = front.to_string();
        self.closed_active = None;
        self.focused(&front);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_example() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        assert_eq!(stack.top(), Some("FTL"));
        stack.started("Void War");
        assert_eq!(stack.top(), Some("FTL"), "a start doesn't jump ahead of a focused game");
        stack.focused("Void War");
        assert_eq!(stack.top(), Some("Void War"));
        stack.focused("FTL");
        assert_eq!(stack.top(), Some("FTL"));
        stack.exited("FTL", t);
        assert_eq!(stack.top(), Some("Void War"));
        assert_eq!(stack.active(), Some("FTL"), "a closed active game stays active");
        stack.exited("Void War", t);
        assert_eq!(stack.top(), None);
        assert_eq!(stack.active(), Some("FTL"));
    }

    #[test]
    fn a_game_brought_forward_by_the_quit_takes_over_only_if_it_stays_in_front() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("Void War");
        stack.started("FTL");
        stack.focused("FTL");
        stack.exited("FTL", t);
        // macOS brings Void War forward.
        stack.focused("Void War");
        assert!(!stack.settle(Some("Void War"), t + Duration::from_secs(1)));
        assert_eq!(stack.active(), Some("FTL"));
        // The user goes to Steam to relaunch: no game in front.
        assert!(!stack.settle(None, t + SETTLE));
        assert_eq!(stack.active(), Some("FTL"));
        // Later the user plays Void War.
        assert!(stack.settle(Some("Void War"), t + SETTLE * 3));
        assert_eq!(stack.active(), Some("Void War"));
    }

    #[test]
    fn a_game_still_in_front_after_the_settle_time_takes_over() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.started("Void War");
        stack.exited("FTL", t);
        stack.focused("Void War");
        assert!(stack.settle(Some("Void War"), t + SETTLE));
        assert_eq!(stack.active(), Some("Void War"));
        assert!(!stack.settle(Some("Void War"), t + SETTLE * 2), "once");
    }

    #[test]
    fn a_game_not_running_in_front_never_takes_over() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.exited("FTL", t);
        // A game waiting for access is in front but kept off the stack.
        assert!(!stack.settle(Some("Waiting"), t + SETTLE * 2));
        assert_eq!(stack.active(), Some("FTL"));
    }

    #[test]
    fn a_closed_active_game_that_starts_again_is_back_on_top() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.started("Void War");
        stack.exited("FTL", t);
        assert!(stack.started("FTL"));
        assert_eq!(stack.entries(), ["FTL", "Void War"]);
        assert_eq!(stack.active(), Some("FTL"));
    }

    #[test]
    fn only_the_active_game_stays_active_after_it_closes() {
        let t = Instant::now();
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.started("Void War");
        stack.exited("Void War", t);
        assert_eq!(stack.active(), Some("FTL"));
        stack.exited("FTL", t);
        stack.started("Void War");
        stack.exited("Void War", t);
        assert_eq!(stack.active(), Some("FTL"), "a start alone doesn't take over");
    }

    #[test]
    fn repeated_starts_are_one_entry() {
        let mut stack = ActiveStack::default();
        assert!(stack.started("a"));
        assert!(!stack.started("a"));
        assert_eq!(stack.entries().len(), 1);
    }
}

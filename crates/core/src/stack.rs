//! The ACTIVE STACK: running games, ordered by which one the user switched
//! to last. And the active game, the hotkeys' target: the game the user was
//! last in, even after it closed.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveStack {
    /// Top first.
    entries: Vec<String>,
    /// The active game after it closed, until another game gets focus.
    closed_active: Option<String>,
}

impl ActiveStack {
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn top(&self) -> Option<&str> {
        self.entries.first().map(String::as_str)
    }

    /// The top, or the game that was on top when it closed, until another
    /// game gets focus.
    pub fn active(&self) -> Option<&str> {
        self.closed_active.as_deref().or_else(|| self.top())
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
        if self.closed_active.as_deref() == Some(game) {
            self.closed_active = None;
            self.entries.insert(0, game.to_string());
        } else {
            self.entries.push(game.to_string());
        }
        true
    }

    /// A game switched to moves to the top and is the active game.
    pub fn focused(&mut self, game: &str) -> bool {
        let was_closed_active = self.closed_active.take().is_some();
        match self.entries.iter().position(|g| g == game) {
            Some(0) => was_closed_active,
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
    pub fn exited(&mut self, game: &str) -> bool {
        if self.active() == Some(game) {
            self.closed_active = Some(game.to_string());
        }
        let before = self.entries.len();
        self.entries.retain(|g| g != game);
        before != self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_example() {
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        assert_eq!(stack.top(), Some("FTL"));
        stack.started("Void War");
        assert_eq!(stack.top(), Some("FTL"), "a start doesn't jump ahead of a focused game");
        stack.focused("Void War");
        assert_eq!(stack.top(), Some("Void War"));
        stack.focused("FTL");
        assert_eq!(stack.top(), Some("FTL"));
        stack.exited("FTL");
        assert_eq!(stack.top(), Some("Void War"));
        assert_eq!(stack.active(), Some("FTL"), "a closed active game stays active");
        stack.focused("Void War");
        assert_eq!(stack.active(), Some("Void War"));
        stack.exited("Void War");
        assert_eq!(stack.top(), None);
        assert_eq!(stack.active(), Some("Void War"));
    }

    #[test]
    fn a_closed_active_game_that_starts_again_is_back_on_top() {
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.started("Void War");
        stack.exited("FTL");
        assert!(stack.started("FTL"));
        assert_eq!(stack.entries(), ["FTL", "Void War"]);
        assert_eq!(stack.active(), Some("FTL"));
    }

    #[test]
    fn only_the_active_game_stays_active_after_it_closes() {
        let mut stack = ActiveStack::default();
        stack.started("FTL");
        stack.started("Void War");
        stack.exited("Void War");
        assert_eq!(stack.active(), Some("FTL"));
        stack.exited("FTL");
        stack.started("Void War");
        stack.exited("Void War");
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

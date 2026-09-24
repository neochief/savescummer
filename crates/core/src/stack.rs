//! The ACTIVE STACK: running games, ordered by which one the user switched
//! to last. Its top is the hotkeys' target.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveStack {
    /// Top first.
    entries: Vec<String>,
}

impl ActiveStack {
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn top(&self) -> Option<&str> {
        self.entries.first().map(String::as_str)
    }

    pub fn contains(&self, game: &str) -> bool {
        self.entries.iter().any(|g| g == game)
    }

    /// A game that starts appears in the stack but doesn't jump ahead of
    /// games focused more recently.
    pub fn started(&mut self, game: &str) -> bool {
        if self.contains(game) {
            return false;
        }
        self.entries.push(game.to_string());
        true
    }

    /// A game switched to moves to the top.
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

    /// When the last process of a game exits, it leaves the stack.
    pub fn exited(&mut self, game: &str) -> bool {
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
        stack.exited("Void War");
        assert_eq!(stack.top(), None);
    }

    #[test]
    fn repeated_starts_are_one_entry() {
        let mut stack = ActiveStack::default();
        assert!(stack.started("a"));
        assert!(!stack.started("a"));
        assert_eq!(stack.entries().len(), 1);
    }
}

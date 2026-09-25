//! Waiting out brief interference (PLAN-HOST, LOAD, "Interference from the
//! running game"): a file action that fails with a transient error is
//! retried within a budget of waiting time. Only the failed action is
//! retried, never the whole operation, and only time spent waiting counts.

use std::cell::Cell;
use std::io;
use std::time::Duration;

use crate::fsx;

/// Waiting allowed across one operation's forward work: the Save or
/// recovery-checkpoint copy and Load stages 1–3.
pub const FORWARD: Duration = Duration::from_secs(1);
/// Waiting allowed for undoing a failed Load; separate, so a forward
/// operation that used its budget up can still be undone.
pub const ROLLBACK: Duration = Duration::from_secs(1);
/// Waiting allowed for deleting the `.ssold` files after a finished Load.
pub const CLEAN_UP: Duration = Duration::from_millis(200);

const FIRST_WAIT: Duration = Duration::from_millis(20);
const LONGEST_WAIT: Duration = Duration::from_millis(200);

/// Time left to wait. Shared by every action that draws on it.
pub struct Budget {
    left: Cell<Duration>,
    next: Cell<Duration>,
    sleep: fn(Duration),
    transient: fn(&io::Error) -> bool,
}

impl Budget {
    pub fn new(total: Duration) -> Budget {
        Budget::custom(total, std::thread::sleep, fsx::is_transient)
    }

    /// A budget with its own waiting and its own idea of a transient error,
    /// for tests.
    pub fn custom(total: Duration, sleep: fn(Duration), transient: fn(&io::Error) -> bool) -> Budget {
        Budget { left: Cell::new(total), next: Cell::new(FIRST_WAIT), sleep, transient }
    }

    pub fn left(&self) -> Duration {
        self.left.get()
    }

    /// Waits a little before the next attempt, a bit longer each time.
    /// False, without waiting, when the budget is used up.
    pub fn wait(&self) -> bool {
        let left = self.left.get();
        if left.is_zero() {
            return false;
        }
        let wait = self.next.get().min(left);
        (self.sleep)(wait);
        self.left.set(left - wait);
        self.next.set((self.next.get() * 2).min(LONGEST_WAIT));
        true
    }

    /// Runs `action`, again after a wait while it fails with a transient
    /// error and the budget lasts. Returns the last result as it is.
    pub fn run<T>(&self, mut action: impl FnMut() -> io::Result<T>) -> io::Result<T> {
        loop {
            match action() {
                Err(e) if (self.transient)(&e) && self.wait() => continue,
                result => return result,
            }
        }
    }
}

/// One operation's budgets: forward work and undoing it.
pub struct Retry {
    pub forward: Budget,
    pub rollback: Budget,
}

impl Retry {
    pub fn new() -> Retry {
        Retry { forward: Budget::new(FORWARD), rollback: Budget::new(ROLLBACK) }
    }
}

impl Default for Retry {
    fn default() -> Retry {
        Retry::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_sleep(_: Duration) {}

    fn transient() -> io::Error {
        io::Error::from(io::ErrorKind::ResourceBusy)
    }

    fn busy(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::ResourceBusy
    }

    fn budget() -> Budget {
        Budget::custom(FORWARD, no_sleep, busy)
    }

    #[test]
    fn a_transient_failure_is_retried_until_it_clears() {
        let budget = budget();
        let mut attempts = 0;
        let result = budget.run(|| {
            attempts += 1;
            if attempts < 3 { Err(transient()) } else { Ok(attempts) }
        });
        assert_eq!(result.unwrap(), 3);
        assert_eq!(budget.left(), FORWARD - FIRST_WAIT - FIRST_WAIT * 2);
    }

    #[test]
    fn other_errors_fail_at_once() {
        let budget = budget();
        let mut attempts = 0;
        let result: io::Result<()> = budget.run(|| {
            attempts += 1;
            Err(io::Error::new(io::ErrorKind::NotFound, "gone"))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(attempts, 1);
        assert_eq!(budget.left(), FORWARD);
    }

    #[test]
    fn a_persistent_failure_returns_its_own_error_once_the_budget_is_used_up() {
        let budget = budget();
        let result: io::Result<()> = budget.run(|| Err(transient()));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ResourceBusy);
        assert!(budget.left().is_zero());
    }

    #[test]
    fn several_actions_share_one_budget() {
        let budget = budget();
        // The first file waits most of the budget out...
        let mut first = 0;
        budget
            .run(|| {
                first += 1;
                if budget.left() > Duration::from_millis(300) { Err(transient()) } else { Ok(()) }
            })
            .unwrap();
        // ...so the second, held just as long, gets only what's left.
        let result: io::Result<()> = budget.run(|| Err(transient()));
        assert!(result.is_err());
        assert!(budget.left().is_zero(), "the total wait stays within the budget");
    }
}

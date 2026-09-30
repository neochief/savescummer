//! One action policy for summaries, command admission and history gates.
//! Facts are observations; gathering them and checking files belongs to callers.

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{Availability, Guidance, GuidanceKind, Phase, Remedy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Save,
    Load,
    Restore,
    Retry,
    Delete,
    Flush,
    Configure,
}

pub struct Facts {
    pub phase: Phase,
    pub busy: bool,
    pub store_moving: bool,
    pub blocked: Option<Failure>,
    pub recovery_failure: Option<Failure>,
    pub store_failure: Option<Failure>,
    pub location_failure: Option<Failure>,
    pub target_failure: Option<Failure>,
    pub exit_locked: bool,
    pub has_data: bool,
    pub has_saves: bool,
}

impl Facts {
    /// Check before reserving the game. Delete waits for an occupied game.
    pub fn admission(&self, action: Action) -> Result<(), Failure> {
        match self.phase {
            Phase::Starting => return Err(failure(ErrorKind::Starting)),
            Phase::ShuttingDown => return Err(failure(ErrorKind::ShuttingDown)),
            Phase::Ready => {}
        }
        if self.store_moving || (self.busy && action != Action::Delete) {
            return Err(failure(ErrorKind::Busy));
        }
        Ok(())
    }

    pub fn check(&self, action: Action) -> Result<(), Failure> {
        self.admission(action)?;
        self.readiness(action)
    }

    pub fn availability(&self, action: Action) -> Availability {
        match self.check(action) {
            Ok(()) => Availability::yes(),
            Err(f) => Availability { available: false, reason: Some(f.kind), failure: Some(f) },
        }
    }

    fn readiness(&self, action: Action) -> Result<(), Failure> {
        if action == Action::Configure {
            return Ok(());
        }
        if self.blocked.is_some() && action != Action::Retry {
            return Err(failure(ErrorKind::Blocked));
        }
        if let Some(f) = &self.store_failure {
            return Err(f.clone());
        }
        if action == Action::Retry {
            if self.blocked.is_none() {
                return Err(failure(ErrorKind::InvalidRequest));
            }
            if let Some(f) = &self.recovery_failure {
                return Err(f.clone());
            }
            // Retry uses journal paths, never the game's newly configured targets.
            return exit_rule(self.exit_locked);
        }
        if matches!(action, Action::Delete | Action::Flush) {
            return Ok(());
        }
        if let Some(f) = &self.location_failure {
            return Err(f.clone());
        }
        if let Some(f) = &self.target_failure {
            return Err(f.clone());
        }
        exit_rule(self.exit_locked)?;
        if action == Action::Save && !self.has_data {
            return Err(failure(ErrorKind::NoGameData));
        }
        if action == Action::Load && !self.has_saves {
            return Err(failure(ErrorKind::NoSaves));
        }
        Ok(())
    }

    /// Persistent guidance deliberately ignores activity. It never authorizes work.
    pub fn guidance(&self) -> Option<Guidance> {
        let mut guidance =
            Guidance { kind: GuidanceKind::Blocked, save: true, load: true, failure: None, remedy: None };
        if let Some(f) = &self.blocked {
            guidance.failure = Some(f.clone());
            guidance.remedy = Some(Remedy::Retry);
            if let Some(access) = self.store_failure.as_ref().or(self.recovery_failure.as_ref())
                && access.kind == ErrorKind::AccessNeeded
            {
                guidance.failure = Some(access.clone());
                guidance.remedy = Some(Remedy::RequestAccess);
            }
            return Some(guidance);
        }
        let problem = self.store_failure.as_ref().or(self.location_failure.as_ref()).or(self.target_failure.as_ref());
        if let Some(f) = problem {
            let (kind, remedy) = match f.kind {
                ErrorKind::AccessNeeded => (GuidanceKind::AccessNeeded, Some(Remedy::RequestAccess)),
                ErrorKind::NoSaveLocation => (GuidanceKind::NoSaveLocation, Some(Remedy::Configure)),
                ErrorKind::InvalidTarget | ErrorKind::InvalidConfig => {
                    (GuidanceKind::InvalidTarget, Some(Remedy::Configure))
                }
                ErrorKind::TargetUnavailable => (GuidanceKind::TargetUnavailable, None),
                _ => return None, // The checkpoint store has its own app-wide notice.
            };
            guidance.kind = kind;
            guidance.failure = Some(f.clone());
            guidance.remedy = remedy;
        } else if self.exit_locked {
            guidance.kind = GuidanceKind::GameRunning;
        } else {
            guidance.save = !self.has_data;
            guidance.load = !self.has_saves;
            guidance.kind = match (self.has_data, self.has_saves) {
                (false, false) => GuidanceKind::PlayFirst,
                (false, true) => GuidanceKind::NoGameData,
                (true, false) => GuidanceKind::NoSaves,
                (true, true) => return None,
            };
        }
        Some(guidance)
    }
}

pub fn exit_rule(locked: bool) -> Result<(), Failure> {
    if locked { Err(failure(ErrorKind::GameRunning)) } else { Ok(()) }
}

fn failure(kind: ErrorKind) -> Failure {
    let detail = match kind {
        ErrorKind::Starting => "the host is still starting",
        ErrorKind::ShuttingDown => "the host is shutting down",
        ErrorKind::Busy => "another operation owns this game or the checkpoint store",
        ErrorKind::Blocked => "the game waits on an interrupted operation",
        ErrorKind::GameRunning => "exit the game before saving, loading or retrying recovery",
        ErrorKind::NoGameData => "no save location matches anything yet",
        ErrorKind::NoSaves => "no usable saved checkpoint to load",
        ErrorKind::InvalidRequest => "the game isn't blocked; send the original command again instead",
        _ => unreachable!(),
    };
    Failure::new(kind, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Facts {
        Facts {
            phase: Phase::Ready,
            busy: false,
            store_moving: false,
            blocked: None,
            recovery_failure: None,
            store_failure: None,
            location_failure: None,
            target_failure: None,
            exit_locked: false,
            has_data: true,
            has_saves: true,
        }
    }

    #[test]
    fn activity_and_stable_guidance_are_independent() {
        let mut facts = ready();
        facts.exit_locked = true;
        facts.busy = true;
        assert_eq!(facts.check(Action::Save).unwrap_err().kind, ErrorKind::Busy);
        assert_eq!(facts.guidance().unwrap().kind, GuidanceKind::GameRunning);
        assert!(facts.check(Action::Delete).is_ok(), "delete may wait while busy");
        facts.busy = false;
        assert_eq!(facts.check(Action::Restore).unwrap_err().kind, ErrorKind::GameRunning);
        assert!(facts.check(Action::Flush).is_ok());
        assert!(facts.check(Action::Configure).is_ok());
    }

    #[test]
    fn recovery_blocks_new_work_but_has_its_own_exit_gate() {
        let mut facts = ready();
        facts.blocked = Some(Failure::new(ErrorKind::RollbackFailed, "kept"));
        facts.exit_locked = true;
        for action in [Action::Save, Action::Load, Action::Restore, Action::Delete, Action::Flush] {
            assert_eq!(facts.check(action).unwrap_err().kind, ErrorKind::Blocked);
        }
        assert_eq!(facts.check(Action::Retry).unwrap_err().kind, ErrorKind::GameRunning);
        assert_eq!(facts.guidance().unwrap().kind, GuidanceKind::Blocked);
        facts.exit_locked = false;
        facts.location_failure = Some(Failure::new(ErrorKind::InvalidTarget, "new location"));
        facts.has_data = false;
        assert!(facts.check(Action::Retry).is_ok(), "recovery uses original journal targets");
        facts.busy = true;
        assert_eq!(facts.check(Action::Retry).unwrap_err().kind, ErrorKind::Busy);
    }

    #[test]
    fn recovery_offers_access_before_retry() {
        let mut facts = ready();
        facts.blocked = Some(Failure::new(ErrorKind::RollbackFailed, "kept"));
        facts.recovery_failure = Some(Failure::new(ErrorKind::AccessNeeded, "documents"));
        assert_eq!(facts.check(Action::Retry).unwrap_err().kind, ErrorKind::AccessNeeded);
        assert_eq!(facts.guidance().unwrap().remedy, Some(Remedy::RequestAccess));
        facts.recovery_failure = None;
        assert!(facts.check(Action::Retry).is_ok());
        assert_eq!(facts.guidance().unwrap().remedy, Some(Remedy::Retry));
    }

    #[test]
    fn reasons_apply_only_to_actions_that_need_them() {
        let mut facts = ready();
        facts.has_data = false;
        assert_eq!(facts.check(Action::Save).unwrap_err().kind, ErrorKind::NoGameData);
        assert!(facts.check(Action::Restore).is_ok());
        facts.has_saves = false;
        assert_eq!(facts.guidance().unwrap().kind, GuidanceKind::PlayFirst);
        assert_eq!(facts.check(Action::Load).unwrap_err().kind, ErrorKind::NoSaves);
        assert!(facts.check(Action::Restore).is_ok(), "a recovery point is independent of main Load");
        facts.location_failure = Some(Failure::new(ErrorKind::NoSaveLocation, "location"));
        facts.exit_locked = true;
        assert_eq!(facts.check(Action::Save).unwrap_err().kind, ErrorKind::NoSaveLocation);
        assert!(facts.check(Action::Delete).is_ok());
        facts.store_failure = Some(Failure::new(ErrorKind::StoreUnavailable, "store"));
        assert_eq!(facts.check(Action::Save).unwrap_err().kind, ErrorKind::StoreUnavailable);
        assert!(facts.check(Action::Configure).is_ok());
    }

    #[test]
    fn save_and_load_can_be_locked_independently() {
        let mut facts = ready();
        facts.has_saves = false;
        assert!(facts.check(Action::Save).is_ok());
        assert_eq!(facts.check(Action::Load).unwrap_err().kind, ErrorKind::NoSaves);

        facts.has_saves = true;
        facts.has_data = false;
        assert_eq!(facts.check(Action::Save).unwrap_err().kind, ErrorKind::NoGameData);
        assert!(facts.check(Action::Load).is_ok());
    }
}

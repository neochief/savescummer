//! Operation cues, played on a dedicated worker so sound never delays file
//! work or holds an operation lock.
//!
//! Cues play one after another with a short gap, so a completion cue never
//! cuts off its start cue even for very fast operations.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    SaveStart,
    SaveDone,
    LoadStart,
    LoadDone,
    Failed,
    Busy,
}

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(windows), path = "unsupported.rs")]
mod imp;

/// Silence between two cues, so back-to-back cues stay distinguishable.
const GAP: Duration = Duration::from_millis(40);
/// At most one busy tick per this interval; extra ones are dropped.
const BUSY_INTERVAL: Duration = Duration::from_millis(400);
/// More pending cues than this means we're backed up: drop the oldest.
const MAX_PENDING: usize = 4;

fn wav(cue: Cue) -> &'static [u8] {
    match cue {
        Cue::SaveStart => include_bytes!("../../../../assets/sounds/save-start.wav"),
        Cue::SaveDone => include_bytes!("../../../../assets/sounds/save-complete.wav"),
        Cue::LoadStart => include_bytes!("../../../../assets/sounds/load-start.wav"),
        Cue::LoadDone => include_bytes!("../../../../assets/sounds/load-complete.wav"),
        Cue::Failed => include_bytes!("../../../../assets/sounds/operation-failed.wav"),
        Cue::Busy => include_bytes!("../../../../assets/sounds/busy.wav"),
    }
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Cue>,
    last_busy: Option<Instant>,
    closed: bool,
}

impl Queue {
    /// Queues `cue` under the rate-limit and backlog rules. Returns whether it
    /// was accepted.
    fn push(&mut self, cue: Cue, now: Instant) -> bool {
        if cue == Cue::Busy {
            if self.last_busy.is_some_and(|last| now.duration_since(last) < BUSY_INTERVAL) {
                return false;
            }
            self.last_busy = Some(now);
        }
        self.pending.push_back(cue);
        // Backed up: drop the oldest cues, but never a failure — the user must
        // always hear that something went wrong.
        while self.pending.len() > MAX_PENDING {
            match self.pending.iter().position(|&c| c != Cue::Failed) {
                Some(i) => {
                    self.pending.remove(i);
                }
                None => break,
            }
        }
        true
    }
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// Plays cues on its own thread; `play` never blocks.
pub struct Player {
    shared: Arc<Shared>,
}

impl Player {
    pub fn new() -> Player {
        let shared = Arc::new(Shared { queue: Mutex::new(Queue::default()), wake: Condvar::new() });
        let worker = Arc::clone(&shared);
        // If the thread can't start there is simply no sound; `play` still
        // only queues.
        let _ = std::thread::Builder::new().name("savescummer-sounds".into()).spawn(move || run(&worker));
        Player { shared }
    }

    pub fn play(&self, cue: Cue) {
        let mut queue = lock(&self.shared.queue);
        if queue.push(cue, Instant::now()) {
            self.shared.wake.notify_one();
        }
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // The worker finishes the cue it's playing (they're short) and exits;
        // queued cues are discarded.
        let mut queue = lock(&self.shared.queue);
        queue.closed = true;
        queue.pending.clear();
        self.shared.wake.notify_one();
    }
}

fn lock(m: &Mutex<Queue>) -> std::sync::MutexGuard<'_, Queue> {
    // A panic elsewhere can't leave the queue inconsistent; keep going.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(shared: &Shared) {
    loop {
        let cue = {
            let mut queue = lock(&shared.queue);
            loop {
                if queue.closed {
                    return;
                }
                if let Some(cue) = queue.pending.pop_front() {
                    break cue;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
        };
        play_now(cue);
    }
}

/// Plays one cue to completion, then leaves the gap.
fn play_now(cue: Cue) {
    imp::play(wav(cue));
    std::thread::sleep(GAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Cue; 6] = [Cue::SaveStart, Cue::SaveDone, Cue::LoadStart, Cue::LoadDone, Cue::Failed, Cue::Busy];

    #[test]
    fn every_cue_is_a_wav() {
        for cue in ALL {
            let bytes = wav(cue);
            assert_eq!(&bytes[..4], b"RIFF", "{cue:?}");
            assert_eq!(&bytes[8..12], b"WAVE", "{cue:?}");
        }
    }

    #[test]
    fn playing_every_cue_never_blocks() {
        let player = Player::new();
        let started = Instant::now();
        for cue in ALL {
            player.play(cue);
        }
        assert!(started.elapsed() < Duration::from_millis(100));
        drop(player);
    }

    #[test]
    fn busy_is_rate_limited() {
        let mut q = Queue::default();
        let t = Instant::now();
        assert!(q.push(Cue::Busy, t));
        assert!(!q.push(Cue::Busy, t + Duration::from_millis(100)));
        assert!(!q.push(Cue::Busy, t + Duration::from_millis(399)));
        assert!(q.push(Cue::Busy, t + Duration::from_millis(400)));
        assert_eq!(q.pending.len(), 2);
    }

    #[test]
    fn backlog_drops_oldest_but_keeps_failures() {
        let mut q = Queue::default();
        let t = Instant::now();
        for cue in [Cue::Failed, Cue::SaveStart, Cue::SaveDone, Cue::LoadStart, Cue::LoadDone, Cue::Failed] {
            q.push(cue, t);
        }
        assert_eq!(Vec::from(q.pending.clone()), vec![Cue::Failed, Cue::LoadStart, Cue::LoadDone, Cue::Failed]);

        let mut q = Queue::default();
        for _ in 0..6 {
            q.push(Cue::Failed, t);
        }
        assert_eq!(q.pending.len(), 6, "failures are never dropped");
    }
}

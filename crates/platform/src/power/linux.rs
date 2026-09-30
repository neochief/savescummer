//! Linux: `CLOCK_MONOTONIC` stops while the machine sleeps and
//! `CLOCK_BOOTTIME` doesn't, so a watcher thread that sees the two drift
//! apart between its checks knows the machine slept. No D-Bus, and it works
//! whoever suspended the machine (logind, the Steam Deck's power button).

use std::time::Duration;

/// How often the watcher looks: a wake is reported at most this late.
const EVERY: Duration = Duration::from_secs(3);
/// More drift than this is a sleep, not scheduling noise.
const SLEPT: Duration = Duration::from_secs(2);

pub fn on_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    let _ = std::thread::Builder::new().name("wake-watch".into()).spawn(move || {
        let mut last = drift();
        loop {
            std::thread::sleep(EVERY);
            let now = drift();
            if now.saturating_sub(last) > SLEPT {
                on_wake();
            }
            last = now;
        }
    });
}

/// How far `CLOCK_BOOTTIME` is ahead of `CLOCK_MONOTONIC`: the time spent
/// asleep since boot.
fn drift() -> Duration {
    clock(libc::CLOCK_BOOTTIME).saturating_sub(clock(libc::CLOCK_MONOTONIC))
}

fn clock(id: libc::clockid_t) -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a plain clock read into a local.
    unsafe { libc::clock_gettime(id, &mut ts) };
    Duration::new(ts.tv_sec.max(0) as u64, ts.tv_nsec.clamp(0, 999_999_999) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_awake_machine_does_not_drift() {
        let before = drift();
        std::thread::sleep(Duration::from_millis(200));
        assert!(drift().saturating_sub(before) < SLEPT);
    }
}

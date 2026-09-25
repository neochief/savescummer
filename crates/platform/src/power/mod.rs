//! Waking from sleep. Timers based on `Instant` don't advance while a Mac
//! sleeps, so the periodic scan runs late after a wake; and a drive may have
//! come or gone meanwhile. The host scans on wake instead.

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(target_os = "macos"), path = "unsupported.rs")]
mod imp;

/// Calls `on_wake` every time the machine wakes from sleep, for the
/// process's lifetime. Where the OS doesn't say (yet), never.
pub fn on_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    imp::on_wake(on_wake)
}

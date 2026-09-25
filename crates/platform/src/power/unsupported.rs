//! Not reported on this OS yet: the periodic scan covers it, late.

pub fn on_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    let _ = on_wake;
}

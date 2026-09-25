//! macOS: `NSWorkspaceDidWakeNotification`, delivered on the main run loop
//! the host always runs.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
use objc2_foundation::NSNotification;

pub fn on_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| on_wake());
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the block only calls the callback, which is Send + Sync.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(NSWorkspaceDidWakeNotification), None, None, &block)
    };
    // Observed for the process's lifetime.
    std::mem::forget(observer);
}

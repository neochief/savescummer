//! Process helpers the host and CLI share: starting a program that outlives
//! its starter, and ending this process the way a crash would.

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "unix.rs")]
mod imp;

pub use imp::{detach, hard_exit};

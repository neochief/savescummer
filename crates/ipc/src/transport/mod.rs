//! The local transport under the protocol: a named pipe on Windows, a Unix
//! socket elsewhere (PLAN-HOST, PROTOCOL). One file per kind; the protocol
//! code on both sides only sees streams.
//!
//! - [`endpoint`]: the pipe or socket name for a host; [`check`] refuses one
//!   the OS can't use (a socket path that's too long).
//! - Clients: [`open`] a blocking [`Stream`]; [`is_busy`] says "try again".
//! - The host: [`Listener::bind`] (inside a tokio runtime), then
//!   [`Listener::accept`] async streams until it fails for good.

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "unix.rs")]
mod imp;

pub use imp::{Listener, ServerStream, Stream, check, endpoint, is_busy, open};

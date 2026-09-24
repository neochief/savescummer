//! The protocol between the host and its clients (PLAN-HOST.md, PROTOCOL):
//! newline-delimited JSON over a named pipe (Windows) or a Unix socket,
//! reachable only by the signed-in user.
//!
//! Every message carries the protocol version. A client sends requests
//! (`{"v":1,"id":"<request id>","type":"save",...}`); the host answers each
//! with a response (`{"v":1,"re":"<request id>","ok":true,"result":{...}}`)
//! and, to watchers, pushes events (`{"v":1,"event":"state","state":{...}}`).

pub mod client;
pub mod types;

pub use client::{Client, ConnectError};
pub use types::*;

use std::path::Path;

pub const PROTOCOL_VERSION: u32 = 1;

/// Largest message either side accepts. A reply that would be bigger is an
/// explicit `too_large` error, never cut off.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// The pipe (or socket) a host for this data folder listens on. One host per
/// user and data folder: tests with their own `--data-dir` get their own.
pub fn endpoint(data_dir: &Path) -> String {
    let normalized = data_dir.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into());
    let user: String = user.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let hash = fnv1a(normalized.as_bytes());
    if cfg!(windows) {
        format!(r"\\.\pipe\savescummer-{user}-{hash:016x}")
    } else {
        // Unix socket paths are short-limited; keep it in the data folder.
        data_dir.join("host.sock").to_string_lossy().into_owned()
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_differ_per_data_folder_and_ignore_spelling() {
        let a = endpoint(Path::new("C:/Data/A"));
        assert_eq!(a, endpoint(Path::new("c:\\data\\a\\")));
        assert_ne!(a, endpoint(Path::new("C:/Data/B")));
    }
}

use std::io;
use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

/// A client's end of the socket.
pub type Stream = std::os::unix::net::UnixStream;
/// The host's end of one connection.
pub type ServerStream = UnixStream;

/// A socket in the data folder, which is already per user.
pub fn endpoint(data_dir: &Path, _name: &str) -> String {
    data_dir.join("host.sock").to_string_lossy().into_owned()
}

/// The longest socket path the OS takes (`sun_path` less its NUL).
const MAX_PATH: usize = if cfg!(target_os = "macos") { 103 } else { 107 };

/// Refuses an endpoint too long for a socket, with the reason in plain words
/// (PLAN-HOST, PROTOCOL: no fallback location).
pub fn check(endpoint: &str) -> io::Result<()> {
    if endpoint.len() > MAX_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the data folder's path is too long for a socket ({} of at most {MAX_PATH} characters in {endpoint}); choose a shorter one",
                endpoint.len()
            ),
        ));
    }
    Ok(())
}

pub fn open(endpoint: &str) -> io::Result<Stream> {
    check(endpoint)?;
    Stream::connect(endpoint)
}

/// A socket never reports "busy".
pub fn is_busy(_e: &io::Error) -> bool {
    false
}

pub struct Listener(UnixListener);

impl Listener {
    /// Binds the socket, for the signed-in user only. Only the host that
    /// holds the data folder's lock gets here (the caller takes it first), so
    /// whatever is at the path is a socket left by a crashed host.
    pub fn bind(endpoint: &str) -> io::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        check(endpoint)?;
        let _ = std::fs::remove_file(endpoint);
        let listener = UnixListener::bind(endpoint)?;
        std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener(listener))
    }

    /// The next connected client of this user. Failed accepts, and anyone
    /// else (who could connect in the moment before the socket's mode was
    /// set), are skipped.
    pub async fn accept(&mut self) -> io::Result<ServerStream> {
        // SAFETY: no preconditions.
        let me = unsafe { libc::geteuid() };
        loop {
            if let Ok((stream, _)) = self.0.accept().await
                && stream.peer_cred().is_ok_and(|cred| cred.uid() == me)
            {
                return Ok(stream);
            }
        }
    }
}

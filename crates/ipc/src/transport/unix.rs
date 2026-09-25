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

pub fn open(endpoint: &str) -> io::Result<Stream> {
    Stream::connect(endpoint)
}

/// A socket never reports "busy".
pub fn is_busy(_e: &io::Error) -> bool {
    false
}

pub struct Listener(UnixListener);

impl Listener {
    /// Binds the socket. Only the host that holds the data folder's lock
    /// gets here, so a socket file left by a crashed host is replaced.
    pub fn bind(endpoint: &str) -> io::Result<Listener> {
        let _ = std::fs::remove_file(endpoint);
        UnixListener::bind(endpoint).map(Listener)
    }

    /// The next connected client. Failed accepts are skipped.
    pub async fn accept(&mut self) -> io::Result<ServerStream> {
        loop {
            if let Ok((stream, _)) = self.0.accept().await {
                return Ok(stream);
            }
        }
    }
}

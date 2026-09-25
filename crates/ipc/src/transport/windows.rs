use std::io;
use std::path::Path;
use std::time::Duration;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// A client's end of the pipe.
pub type Stream = std::fs::File;
/// The host's end of one connection.
pub type ServerStream = NamedPipeServer;

/// A named pipe, per user and data folder.
pub fn endpoint(_data_dir: &Path, name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

pub fn open(endpoint: &str) -> io::Result<Stream> {
    std::fs::OpenOptions::new().read(true).write(true).open(endpoint)
}

/// Every pipe instance is taken for a moment (`ERROR_PIPE_BUSY`).
pub fn is_busy(e: &io::Error) -> bool {
    e.raw_os_error() == Some(231)
}

/// The pipe instance waiting for the next client.
pub struct Listener {
    endpoint: String,
    next: NamedPipeServer,
}

impl Listener {
    /// Creates the first pipe instance; fails when another host owns the name.
    pub fn bind(endpoint: &str) -> io::Result<Listener> {
        let next = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(endpoint)?;
        Ok(Listener { endpoint: endpoint.to_string(), next })
    }

    /// The next connected client. An error is final: no new pipe instance
    /// could be created.
    pub async fn accept(&mut self) -> io::Result<ServerStream> {
        if self.next.connect().await.is_err() {
            // The client went away before we connected; its instance just
            // reads end-of-file.
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let next = ServerOptions::new().reject_remote_clients(true).create(&self.endpoint)?;
        Ok(std::mem::replace(&mut self.next, next))
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoint;
    use std::path::Path;

    #[test]
    fn pipe_names_ignore_spelling() {
        let a = endpoint(Path::new("C:/Data/A"));
        assert_eq!(a, endpoint(Path::new("c:\\data\\a\\")));
        assert_ne!(a, endpoint(Path::new("C:/Data/B")));
    }
}

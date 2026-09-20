use savescummer_core::*;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const VERSION: u32 = 2;
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub request_id: Id,
    pub command: Command,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    State,
    Watch,
    CheckArtwork,
    SetPlaySounds {
        enabled: bool,
    },
    SetLaunchOnStartup {
        enabled: bool,
    },
    ExecuteActive {
        action: ShortcutAction,
    },
    ExplorerTargets {
        path: PathBuf,
    },
    Explore {
        game_id: Id,
    },
    /// None resets to the sole detected location; ambiguous games require a choice.
    SelectDetectedLocation {
        game_id: Id,
        location: Option<GameLocation>,
    },
    Configure {
        id: Id,
        name: String,
        data_dir: PathBuf,
        #[serde(default)]
        executables: Vec<PathBuf>,
    },
    History {
        game_id: Id,
    },
    Execute {
        game_id: Id,
        action: Action,
    },
    Operation {
        operation_id: Id,
    },
    FlushPreview {
        game_id: Id,
    },
    Rescan,
    Shutdown,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub request_id: Id,
    pub host_id: Id,
    pub result: Reply,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    State { state: State },
    Accepted { operation_id: Id },
    Operation { operation: Box<Operation> },
    Configured { game: Game },
    FlushPreview { preview: FlushPreview },
    ExplorerTargets { targets: Vec<ExplorerTarget> },
    ShuttingDown,
    Error { error: Error },
    Ok,
}
pub async fn read_frame<T: serde::de::DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
) -> io::Result<T> {
    let size = stream.read_u32_le().await? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub async fn write_frame<T: Serialize>(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response exceeds frame limit",
        ));
    }
    stream.write_u32_le(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{Listener, connect, user_id};
#[cfg(unix)]
pub fn user_id() -> io::Result<String> {
    Ok(std::env::var("USER").unwrap_or_else(|_| "user".into()))
}

pub fn endpoint(data_dir: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let canonical = std::fs::canonicalize(data_dir)?;
        let text = canonical.to_string_lossy().to_lowercase();
        let hash = text.as_bytes().iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        Ok(PathBuf::from(format!(
            r"\\.\pipe\savescummer-v{VERSION}-{}-{hash:x}",
            user_id()?
        )))
    }
    #[cfg(unix)]
    {
        Ok(data_dir.join("host.sock"))
    }
}
#[cfg(unix)]
pub struct Listener {
    listener: tokio::net::UnixListener,
    path: PathBuf,
}
#[cfg(unix)]
impl Listener {
    /// Caller must hold the data-directory instance lock before removing a stale socket.
    pub fn bind(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }
    pub async fn accept(&mut self) -> io::Result<tokio::net::UnixStream> {
        self.listener.accept().await.map(|(stream, _)| stream)
    }
}
#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
#[cfg(unix)]
pub async fn connect(path: &Path) -> io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(path).await
}

pub async fn request(path: &Path, request: &Request) -> io::Result<Response> {
    let mut stream = connect(path).await?;
    write_frame(&mut stream, request).await?;
    let response: Response = read_frame(&mut stream).await?;
    if response.version != VERSION || response.request_id != request.request_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response version or request ID mismatch",
        ));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_fixtures() {
        for fixture in [
            include_str!("../../../protocol/fixtures/state-request.json"),
            include_str!("../../../protocol/fixtures/artwork-request.json"),
            include_str!("../../../protocol/fixtures/save-request.json"),
            include_str!("../../../protocol/fixtures/load-request.json"),
            include_str!("../../../protocol/fixtures/revert-request.json"),
            include_str!("../../../protocol/fixtures/sounds-request.json"),
            include_str!("../../../protocol/fixtures/startup-request.json"),
            include_str!("../../../protocol/fixtures/active-request.json"),
            include_str!("../../../protocol/fixtures/explorer-request.json"),
            include_str!("../../../protocol/fixtures/reset-request.json"),
        ] {
            let request: Request = serde_json::from_str(fixture).unwrap();
            assert_eq!(request.version, VERSION);
            let decoded: serde_json::Value = serde_json::from_str(fixture).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), decoded);
        }
    }
    #[test]
    fn checked_in_schemas_match_wire_types() {
        for (text, generated) in [
            (
                include_str!("../../../protocol/request.schema.json"),
                schemars::schema_for!(Request),
            ),
            (
                include_str!("../../../protocol/response.schema.json"),
                schemars::schema_for!(Response),
            ),
        ] {
            let checked_in: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(
                checked_in,
                serde_json::to_value(generated).unwrap(),
                "regenerate and review the shared protocol schema"
            );
        }
    }
    #[tokio::test]
    async fn rejects_oversized_frames_before_allocation() {
        let (mut a, mut b) = tokio::io::duplex(32);
        a.write_u32_le((MAX_FRAME + 1) as u32).await.unwrap();
        assert_eq!(
            read_frame::<Request>(&mut b).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

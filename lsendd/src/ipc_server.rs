use ipc::{Request, Response, StatusResponse, read_message, socket_path, write_message};
use std::path::PathBuf;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};

/// The Unix domain socket `lsendctl` connects to.
pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
}

impl IpcServer {
    pub async fn bind() -> anyhow::Result<Self> {
        let path = socket_path()?;
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        // Remove a leftover socket file from a previous, uncleanly stopped run.
        let _ = tokio::fs::remove_file(&path).await;
        let listener = UnixListener::bind(&path)?;
        Ok(Self { listener, path })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub async fn accept(&self) -> std::io::Result<UnixStream> {
        Ok(self.listener.accept().await?.0)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Handles a single IPC connection: reads one request, writes one response.
pub async fn handle_connection(stream: UnixStream, status: StatusResponse) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let response = match read_message::<_, Request>(&mut reader).await? {
        Some(Request::Status) => Response::Status(status),
        None => return Ok(()),
    };
    write_message(&mut write_half, &response).await
}

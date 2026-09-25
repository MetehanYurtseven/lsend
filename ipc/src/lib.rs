use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Status,
    List,
    Send { target: String, paths: Vec<PathBuf> },
    Pending,
    Accept { id: u64 },
    Decline { id: u64 },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status(StatusResponse),
    List {
        devices: Vec<DeviceEntry>,
    },
    Send {
        sent_files: usize,
    },
    Pending {
        requests: Vec<PendingEntry>,
    },
    /// `text` is set when the accepted request was a text message.
    Accept {
        text: Option<String>,
    },
    Decline,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub alias: String,
    pub fingerprint: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub alias: String,
    pub fingerprint: String,
    pub address: String,
    pub device_type: Option<DeviceType>,
}

/// An incoming request waiting for `accept` or `decline`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEntry {
    pub id: u64,
    pub alias: String,
    /// Verified by the TLS handshake, the value to put into the `known` file.
    pub fingerprint: String,
    pub address: String,
    #[serde(flatten)]
    pub content: PendingContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingContent {
    Files { count: usize },
    Text,
}

impl std::fmt::Display for PendingContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Files { count } => write!(formatter, "{count} files"),
            Self::Text => formatter.write_str("text"),
        }
    }
}

/// Mirrors `localsend::model::discovery::DeviceType` so that `lsendctl` does
/// not need to depend on the `localsend` core crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Mobile,
    Desktop,
    Web,
    Headless,
    Server,
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Mobile => "mobile",
            Self::Desktop => "desktop",
            Self::Web => "web",
            Self::Headless => "headless",
            Self::Server => "server",
        })
    }
}

/// The Unix domain socket lsendd listens on and lsendctl connects to.
pub fn socket_path() -> anyhow::Result<PathBuf> {
    let runtime_dir =
        env::var("XDG_RUNTIME_DIR").map_err(|_| anyhow::anyhow!("XDG_RUNTIME_DIR is not set"))?;
    Ok(PathBuf::from(runtime_dir).join("lsend").join("lsendd.sock"))
}

/// Writes a single newline-delimited JSON message.
pub async fn write_message<W, T>(writer: &mut W, message: &T) -> anyhow::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    Ok(())
}

/// Reads a single newline-delimited JSON message. Returns `Ok(None)` on EOF
/// (peer closed the connection before sending anything).
pub async fn read_message<R, T>(reader: &mut BufReader<R>) -> anyhow::Result<Option<T>>
where
    R: tokio::io::AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(line.trim_end())?))
}

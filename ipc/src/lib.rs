use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status(StatusResponse),
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub alias: String,
    pub fingerprint: String,
    pub port: u16,
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

use ipc::{
    DeviceEntry, DeviceType, Request, Response, StatusResponse, read_message, socket_path,
    write_message,
};
use localsend::discovery::DiscoveryHandle;
use std::path::PathBuf;
use std::sync::Arc;
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
pub async fn handle_connection(
    stream: UnixStream,
    status: StatusResponse,
    discovery: Arc<DiscoveryHandle>,
) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let response = match read_message::<_, Request>(&mut reader).await? {
        Some(Request::Status) => Response::Status(status),
        Some(Request::List) => Response::List {
            devices: list_devices(&discovery),
        },
        None => return Ok(()),
    };
    write_message(&mut write_half, &response).await
}

fn list_devices(discovery: &DiscoveryHandle) -> Vec<DeviceEntry> {
    discovery
        .devices()
        .into_iter()
        .map(|known| {
            let address = known
                .get_best_channel()
                .and_then(|channel| channel.http())
                .map(|http| format!("{}:{}", http.host, http.port))
                .unwrap_or_default();
            DeviceEntry {
                alias: known.device.alias,
                fingerprint: known.device.fingerprint,
                address,
                device_type: known.device.device_type.map(map_device_type),
            }
        })
        .collect()
}

fn map_device_type(device_type: localsend::model::discovery::DeviceType) -> DeviceType {
    use localsend::model::discovery::DeviceType as CoreDeviceType;
    match device_type {
        CoreDeviceType::Mobile => DeviceType::Mobile,
        CoreDeviceType::Desktop => DeviceType::Desktop,
        CoreDeviceType::Web => DeviceType::Web,
        CoreDeviceType::Headless => DeviceType::Headless,
        CoreDeviceType::Server => DeviceType::Server,
    }
}

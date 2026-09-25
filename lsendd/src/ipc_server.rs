use crate::identity::Identity;
use crate::send;
use ipc::{
    DeviceEntry, DeviceType, Request, Response, StatusResponse, read_message, socket_path,
    write_message,
};
use localsend::discovery::{DiscoveryHandle, StatefulDevice};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::{TcpStream, UnixListener, UnixStream};

/// How long `list` waits for a device to answer a TCP connect before giving
/// up on it.
const LIST_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

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
    identity: Arc<Identity>,
    discovery: Arc<DiscoveryHandle>,
) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let response = match read_message::<_, Request>(&mut reader).await? {
        Some(Request::Status) => Response::Status(status),
        Some(Request::List) => Response::List {
            devices: list_devices(&discovery).await,
        },
        Some(Request::Send { target, paths }) => {
            match send::send(&identity, &discovery, &target, paths).await {
                Ok(sent_files) => Response::Send { sent_files },
                Err(message) => Response::Error { message },
            }
        }
        None => return Ok(()),
    };
    write_message(&mut write_half, &response).await
}

/// Returns the known devices that are currently reachable: a plain TCP
/// connect to each device's best channel, in parallel, no LocalSend protocol
/// involved. Devices that no longer answer are left out, without touching
/// the underlying discovery store.
async fn list_devices(discovery: &DiscoveryHandle) -> Vec<DeviceEntry> {
    let mut checks = tokio::task::JoinSet::new();
    for known in discovery.devices() {
        checks.spawn(async move { is_reachable(&known).await.then(|| to_entry(known)) });
    }

    let mut devices = Vec::new();
    while let Some(result) = checks.join_next().await {
        if let Ok(Some(entry)) = result {
            devices.push(entry);
        }
    }
    devices
}

async fn is_reachable(known: &StatefulDevice) -> bool {
    let Some(http) = known.get_best_channel().and_then(|channel| channel.http()) else {
        return false;
    };
    tokio::time::timeout(
        LIST_PROBE_TIMEOUT,
        TcpStream::connect((http.host.as_str(), http.port)),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

fn to_entry(known: StatefulDevice) -> DeviceEntry {
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

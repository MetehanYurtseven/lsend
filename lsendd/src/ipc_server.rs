use crate::identity::Identity;
use crate::send;
use ipc::{
    DeviceEntry, DeviceType, Request, Response, StatusResponse, read_message, socket_path,
    write_message,
};
use localsend::discovery::DiscoveryHandle;
use localsend::model::discovery::ProtocolType;
use localsend::util::interface::{InterfaceFilter, local_interface_addresses};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};

/// How long a `list` scan waits for devices to answer before returning.
const LIST_SCAN_GRACE: Duration = Duration::from_secs(1);

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
            devices: list_devices(&identity, &discovery).await,
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

/// Scans the network and returns only the devices that answered this scan,
/// so a device that has gone offline since it was last seen does not linger.
async fn list_devices(identity: &Identity, discovery: &DiscoveryHandle) -> Vec<DeviceEntry> {
    let scan_start = SystemTime::now();
    let interface_ips = local_interface_addresses(&InterfaceFilter::default()).unwrap_or_default();
    if let Err(err) = discovery
        .discover_staged(
            Vec::new(),
            interface_ips,
            identity.port,
            ProtocolType::Https,
            LIST_SCAN_GRACE,
        )
        .await
    {
        eprintln!("List scan failed: {err}");
    }

    discovery
        .devices()
        .into_iter()
        .filter(|known| {
            known
                .logs
                .last()
                .is_some_and(|log| log.timestamp >= scan_start)
        })
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

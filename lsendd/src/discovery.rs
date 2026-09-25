use crate::identity::Identity;
use localsend::discovery::{
    DEFAULT_DISCOVERY_TIMEOUT, DeviceIdentity, DiscoveryConfig, DiscoveryHandle,
};
use localsend::multicast::{DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6};
use localsend::util::interface::InterfaceFilter;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// Running discovery: multicast announce/listen, so peers learn about this
/// device.
pub struct Discovery {
    pub handle: Arc<DiscoveryHandle>,
    stop_tx: oneshot::Sender<()>,
}

impl Discovery {
    pub async fn start(identity: &Identity) -> Self {
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let handle = localsend::discovery::start(
            DiscoveryConfig {
                group: DEFAULT_MULTICAST_GROUP,
                group_v6: Some(DEFAULT_MULTICAST_GROUP_V6),
                port: identity.port,
                interface_filter: InterfaceFilter::default(),
                device: identity.multicast_device(),
                identity: DeviceIdentity {
                    cert_pem: identity.cert.certificate_pem.clone(),
                    private_key_pem: identity.cert.private_key_pem.clone(),
                },
                timeout: DEFAULT_DISCOVERY_TIMEOUT,
                event_tx: None,
            },
            stop_rx,
        )
        .await;

        if let Some(err) = handle.multicast_error() {
            eprintln!("Multicast unavailable: {err:#}");
        }

        Self {
            handle: Arc::new(handle),
            stop_tx,
        }
    }

    pub async fn announce(&self) {
        self.handle.announce().await;
    }

    pub async fn shutdown(self) {
        let _ = self.stop_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(1), self.handle.wait_stopped()).await;
    }
}

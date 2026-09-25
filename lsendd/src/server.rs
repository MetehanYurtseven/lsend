use localsend::crypto::cert::SelfSignedCert;
use localsend::http::server::v2::ServerEventV2;
use localsend::http::server::web::WebConfig;
use localsend::http::server::{ServerConfigV2, ServerHandle, TlsConfig, start_with_port};
use localsend::http::state::ClientInfo;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// A running HTTP server: accepts register/prepare-upload/... requests from
/// peers.
pub struct Server {
    pub handle: Arc<ServerHandle>,
    pub events: mpsc::Receiver<ServerEventV2>,
    stop_tx: oneshot::Sender<()>,
}

impl Server {
    pub async fn start(cert: &SelfSignedCert, alias: &str, port: u16) -> anyhow::Result<Self> {
        let (event_tx, events) = mpsc::channel::<ServerEventV2>(16);
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let handle = start_with_port(
            port,
            Some(TlsConfig {
                cert: cert.certificate_pem.clone(),
                private_key: cert.private_key_pem.clone(),
            }),
            ClientInfo {
                alias: alias.to_string(),
                version: PROTOCOL_VERSION_V2.to_string(),
                device_model: Some("lsendd".to_string()),
                device_type: Some(DeviceType::Headless),
                token: cert.fingerprint.clone(),
            },
            None,
            Some(ServerConfigV2 {
                pin: None,
                verify_checksums: true,
                event_tx,
            }),
            WebConfig::default(),
            stop_rx,
        )
        .await?;

        Ok(Self {
            handle: Arc::new(handle),
            events,
            stop_tx,
        })
    }

    pub async fn shutdown(self) {
        let _ = self.stop_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(1), self.handle.wait_stopped()).await;
    }
}

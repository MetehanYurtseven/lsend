use crate::identity::Identity;
use localsend::http::server::v2::ServerEventV2;
use localsend::http::server::web::WebConfig;
use localsend::http::server::{ServerConfigV2, ServerHandle, TlsConfig, start_with_port};
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
    pub async fn start(identity: &Identity) -> anyhow::Result<Self> {
        let (event_tx, events) = mpsc::channel::<ServerEventV2>(16);
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let handle = start_with_port(
            identity.port,
            Some(TlsConfig {
                cert: identity.cert.certificate_pem.clone(),
                private_key: identity.cert.private_key_pem.clone(),
            }),
            identity.client_info(),
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

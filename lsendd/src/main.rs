use localsend::crypto::cert::generate_self_signed;
use localsend::discovery::{DEFAULT_DISCOVERY_TIMEOUT, DeviceIdentity, DiscoveryConfig};
use localsend::http::server::v2::{PrepareUploadDecisionV2, ServerEventV2};
use localsend::http::server::web::WebConfig;
use localsend::http::server::{ServerConfigV2, TlsConfig, start_with_port};
use localsend::http::state::ClientInfo;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2, ProtocolType};
use localsend::multicast::{
    DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6, DEFAULT_PORT, MulticastDevice,
};
use localsend::util::interface::InterfaceFilter;
use tokio::sync::{mpsc, oneshot};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cert = generate_self_signed()?;
    println!("Generated identity, fingerprint: {}", cert.fingerprint);

    let alias = "lsendd-test".to_string();
    let port = DEFAULT_PORT;

    // HTTP server: accepts register/prepare-upload/... requests from peers.
    let (server_tx, mut server_rx) = mpsc::channel::<ServerEventV2>(16);
    let (server_stop_tx, server_stop_rx) = oneshot::channel::<()>();
    let server = start_with_port(
        port,
        Some(TlsConfig {
            cert: cert.certificate_pem.clone(),
            private_key: cert.private_key_pem.clone(),
        }),
        ClientInfo {
            alias: alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some("lsendd".to_string()),
            device_type: Some(DeviceType::Headless),
            token: cert.fingerprint.clone(),
        },
        None,
        Some(ServerConfigV2 {
            pin: None,
            verify_checksums: true,
            event_tx: server_tx,
        }),
        WebConfig::default(),
        server_stop_rx,
    )
    .await?;
    println!("HTTP server listening on port {port}");

    // Discovery: multicast announce/listen, so peers learn about this device.
    let device = MulticastDevice {
        alias,
        version: PROTOCOL_VERSION_V2.to_string(),
        device_model: Some("lsendd".to_string()),
        device_type: Some(DeviceType::Headless),
        fingerprint: cert.fingerprint.clone(),
        port,
        protocol: ProtocolType::Https,
        download: false,
    };
    let (discovery_stop_tx, discovery_stop_rx) = oneshot::channel::<()>();
    let discovery = localsend::discovery::start(
        DiscoveryConfig {
            group: DEFAULT_MULTICAST_GROUP,
            group_v6: Some(DEFAULT_MULTICAST_GROUP_V6),
            port: DEFAULT_PORT,
            interface_filter: InterfaceFilter::default(),
            device,
            identity: DeviceIdentity {
                cert_pem: cert.certificate_pem.clone(),
                private_key_pem: cert.private_key_pem.clone(),
            },
            timeout: DEFAULT_DISCOVERY_TIMEOUT,
            event_tx: None,
        },
        discovery_stop_rx,
    )
    .await;

    if let Some(err) = discovery.multicast_error() {
        eprintln!("Multicast unavailable: {err:#}");
    }

    println!("Announcing on the network...");
    discovery.announce().await;

    println!("Running. Press Ctrl+C to stop.");
    loop {
        tokio::select! {
            Some(event) = server_rx.recv() => handle_server_event(event),
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    println!("Shutting down...");
    let _ = server_stop_tx.send(());
    let _ = discovery_stop_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), server.wait_stopped()).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), discovery.wait_stopped()).await;
    println!("Stopped.");

    Ok(())
}

/// Handles an incoming server event. No auto-accept exists in the core
/// crate, so prepare-upload requests are declined until `lsendd` grows a
/// real accept policy.
fn handle_server_event(event: ServerEventV2) {
    match event {
        ServerEventV2::Register { ip, info } => {
            println!("Register from {ip}: {} ({})", info.alias, info.fingerprint);
        }
        ServerEventV2::PrepareUpload {
            ip,
            info,
            decision_tx,
            ..
        } => {
            println!("PrepareUpload from {ip} ({}): declining", info.alias);
            let _ = decision_tx.send(PrepareUploadDecisionV2::Decline);
        }
        ServerEventV2::FileUpload { target_tx, .. } => {
            // Never reached: no PrepareUpload is ever accepted above.
            drop(target_tx);
        }
        ServerEventV2::SessionEnd { session_id, reason } => {
            println!("SessionEnd {session_id}: {reason:?}");
        }
        ServerEventV2::PrepareUploadAborted { session_id } => {
            println!("PrepareUploadAborted {session_id}");
        }
        ServerEventV2::CancelReceived { ip, session_id } => {
            println!("CancelReceived from {ip}: {session_id}");
        }
        ServerEventV2::ListenerFailed { error } => {
            eprintln!("Server listener failed: {error}");
        }
    }
}

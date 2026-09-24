use localsend::crypto::cert::generate_self_signed;
use localsend::discovery::{DEFAULT_DISCOVERY_TIMEOUT, DeviceIdentity, DiscoveryConfig};
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2, ProtocolType};
use localsend::multicast::{
    DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6, DEFAULT_PORT, MulticastDevice,
};
use localsend::util::interface::InterfaceFilter;
use std::time::Duration;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cert = generate_self_signed()?;
    println!("Generated identity, fingerprint: {}", cert.fingerprint);

    let device = MulticastDevice {
        alias: "lsendd-test".to_string(),
        version: PROTOCOL_VERSION_V2.to_string(),
        device_model: Some("lsendd".to_string()),
        device_type: Some(DeviceType::Headless),
        fingerprint: cert.fingerprint.clone(),
        port: DEFAULT_PORT,
        protocol: ProtocolType::Https,
        download: false,
    };

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
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
        stop_rx,
    )
    .await;

    if let Some(err) = discovery.multicast_error() {
        eprintln!("Multicast unavailable: {err:#}");
    }

    println!("Announcing on the network...");
    discovery.announce().await;

    println!("Waiting 3s for replies...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    for device in discovery.devices() {
        println!(
            "Found: {} ({})",
            device.device.alias, device.device.fingerprint
        );
    }

    let _ = stop_tx.send(());
    discovery.wait_stopped().await;
    println!("Stopped.");

    Ok(())
}

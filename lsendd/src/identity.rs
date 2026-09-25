use ipc::StatusResponse;
use localsend::crypto::cert::{SelfSignedCert, generate_self_signed};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::http::state::ClientInfo;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2, ProtocolType};
use localsend::multicast::MulticastDevice;

const DEVICE_MODEL: &str = "lsendd";
const DEVICE_TYPE: DeviceType = DeviceType::Headless;
const PROTOCOL: ProtocolType = ProtocolType::Https;

/// This device's identity: alias, port and certificate/key pair. The single
/// source for everything this device tells its peers about itself.
pub struct Identity {
    pub alias: String,
    pub port: u16,
    pub cert: SelfSignedCert,
}

impl Identity {
    /// Generates a fresh identity (RSA-2048 key pair + self-signed
    /// certificate). Not yet persisted: a new identity, and thus fingerprint,
    /// is generated on every daemon start.
    pub fn generate(alias: String, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            alias,
            port,
            cert: generate_self_signed()?,
        })
    }

    pub fn fingerprint(&self) -> &str {
        &self.cert.fingerprint
    }

    /// What discovery announces via multicast.
    pub fn multicast_device(&self) -> MulticastDevice {
        MulticastDevice {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some(DEVICE_MODEL.to_string()),
            device_type: Some(DEVICE_TYPE),
            fingerprint: self.cert.fingerprint.clone(),
            port: self.port,
            protocol: PROTOCOL,
            download: false,
        }
    }

    /// What the HTTP server answers to peers.
    pub fn client_info(&self) -> ClientInfo {
        ClientInfo {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some(DEVICE_MODEL.to_string()),
            device_type: Some(DEVICE_TYPE),
            token: self.cert.fingerprint.clone(),
        }
    }

    /// What the HTTP client sends to peers.
    pub fn register_dto(&self) -> RegisterDtoV2 {
        RegisterDtoV2 {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some(DEVICE_MODEL.to_string()),
            device_type: Some(DEVICE_TYPE),
            fingerprint: self.cert.fingerprint.clone(),
            port: self.port,
            protocol: PROTOCOL,
            download: false,
        }
    }

    pub fn status(&self) -> StatusResponse {
        StatusResponse {
            alias: self.alias.clone(),
            fingerprint: self.cert.fingerprint.clone(),
            port: self.port,
        }
    }
}

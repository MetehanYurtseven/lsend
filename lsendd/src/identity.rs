use localsend::crypto::cert::{SelfSignedCert, generate_self_signed};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2, ProtocolType};

/// Generates a fresh device identity (RSA-2048 keypair + self-signed
/// certificate). Not yet persisted: a new identity, and thus fingerprint, is
/// generated on every daemon start.
pub fn generate() -> anyhow::Result<SelfSignedCert> {
    generate_self_signed()
}

/// This device's identity as needed to act as an HTTP client towards other
/// devices: alias, port and the certificate/key pair from [`generate`].
pub struct Identity {
    pub alias: String,
    pub port: u16,
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
}

impl Identity {
    pub fn new(cert: &SelfSignedCert, alias: String, port: u16) -> Self {
        Self {
            alias,
            port,
            cert_pem: cert.certificate_pem.clone(),
            key_pem: cert.private_key_pem.clone(),
            fingerprint: cert.fingerprint.clone(),
        }
    }

    pub fn register_dto(&self) -> RegisterDtoV2 {
        RegisterDtoV2 {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some("lsendd".to_string()),
            device_type: Some(DeviceType::Headless),
            fingerprint: self.fingerprint.clone(),
            port: self.port,
            protocol: ProtocolType::Https,
            download: false,
        }
    }
}

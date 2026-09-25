use anyhow::Context;
use ipc::StatusResponse;
use localsend::crypto::cert::{
    SelfSignedCert, fingerprint_from_cert_der, generate_self_signed, public_key_from_cert_der,
};
use localsend::http::dto_v2::RegisterDtoV2;
use localsend::http::state::ClientInfo;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2, ProtocolType};
use localsend::multicast::MulticastDevice;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

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
    /// Where the certificate and private key are persisted, so the
    /// fingerprint survives restarts.
    pub fn path() -> anyhow::Result<PathBuf> {
        let dir = dirs::state_dir().context("Could not determine XDG state directory")?;
        Ok(dir.join("lsend").join("identity.pem"))
    }

    /// Loads the identity from `path`, generating and saving a fresh one
    /// (RSA-2048 key pair + self-signed certificate) if it does not exist yet.
    pub fn load_or_generate(path: &Path, alias: String, port: u16) -> anyhow::Result<Self> {
        let cert = match std::fs::read_to_string(path) {
            Ok(text) => parse(&text).with_context(|| {
                format!(
                    "Invalid identity file {} (delete it to generate a new identity)",
                    path.display()
                )
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let cert = generate_self_signed()?;
                save(path, &cert).with_context(|| format!("Could not save {}", path.display()))?;
                cert
            }
            Err(err) => return Err(err).context(format!("Could not read {}", path.display())),
        };
        Ok(Self { alias, port, cert })
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

fn parse(text: &str) -> anyhow::Result<SelfSignedCert> {
    let blocks = pem::parse_many(text)?;
    let cert = blocks
        .iter()
        .find(|block| block.tag() == "CERTIFICATE")
        .context("missing CERTIFICATE block")?;
    let key = blocks
        .iter()
        .find(|block| block.tag() == "PRIVATE KEY")
        .context("missing PRIVATE KEY block")?;
    Ok(SelfSignedCert {
        private_key_pem: pem::encode(key),
        public_key_pem: public_key_from_cert_der(cert.contents())?,
        certificate_pem: pem::encode(cert),
        fingerprint: fingerprint_from_cert_der(cert.contents()),
    })
}

fn save(path: &Path, cert: &SelfSignedCert) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // The file contains the private key; keep it owner-readable only.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(format!("{}{}", cert.certificate_pem, cert.private_key_pem).as_bytes())?;
    Ok(())
}

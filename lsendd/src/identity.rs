use localsend::crypto::cert::{SelfSignedCert, generate_self_signed};

/// Generates a fresh device identity (RSA-2048 keypair + self-signed
/// certificate). Not yet persisted: a new identity, and thus fingerprint, is
/// generated on every daemon start.
pub fn generate() -> anyhow::Result<SelfSignedCert> {
    generate_self_signed()
}

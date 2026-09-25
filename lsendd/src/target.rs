use localsend::discovery::{DiscoveryHandle, StatefulDevice};
use localsend::model::discovery::ProtocolType;
use localsend::multicast::DEFAULT_PORT;
use std::net::IpAddr;

/// A `send --to` destination: either a device alias (looked up in the
/// discovery store) or an IP address (asked directly who it is).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSelector {
    Alias(String),
    Ip(IpAddr),
}

impl TargetSelector {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("Destination cannot be empty".to_string());
        }
        Ok(match value.parse::<IpAddr>() {
            Ok(ip) => Self::Ip(ip),
            Err(_) => Self::Alias(value.to_string()),
        })
    }

    /// Resolves the destination without scanning the network: an alias is
    /// looked up among the already known devices, an IP address gets a single
    /// register request.
    pub async fn resolve(&self, discovery: &DiscoveryHandle) -> Result<StatefulDevice, String> {
        match self {
            Self::Alias(alias) => {
                let devices = discovery.devices();
                let matching: Vec<&StatefulDevice> = devices
                    .iter()
                    .filter(|device| device.device.alias == *alias)
                    .collect();
                match matching.as_slice() {
                    [] => Err(format!("Destination {self} was not discovered")),
                    [device] => Ok((*device).clone()),
                    devices => Err(format!(
                        "Destination {self} is ambiguous ({} devices matched); use an IP address",
                        devices.len()
                    )),
                }
            }
            Self::Ip(ip) => discovery
                .discover(&ip.to_string(), DEFAULT_PORT, ProtocolType::Https)
                .await
                .map_err(|err| format!("Destination {self} did not answer: {err}"))?
                .ok_or_else(|| format!("Destination {self} is this device")),
        }
    }
}

impl std::fmt::Display for TargetSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alias(alias) => write!(formatter, "alias {alias:?}"),
            Self::Ip(ip) => write!(formatter, "IP address {ip}"),
        }
    }
}

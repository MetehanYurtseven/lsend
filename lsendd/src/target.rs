use localsend::discovery::{HttpChannel, StatefulDevice};
use localsend::model::discovery::ProtocolType;
use localsend::multicast::DEFAULT_PORT;
use std::net::IpAddr;

/// A `send --to` destination: either a device alias (resolved against the
/// discovery store) or an IP address (dialed directly, no discovery lookup
/// needed to find it, only to identify it).
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

    /// A channel to probe directly, so an IP destination is found even when
    /// multicast does not reach it.
    pub fn direct_channel(&self) -> Option<HttpChannel> {
        let Self::Ip(ip) = self else {
            return None;
        };
        Some(HttpChannel {
            host: ip.to_string(),
            port: DEFAULT_PORT,
            protocol: ProtocolType::Https,
        })
    }

    pub fn resolve(&self, devices: &[StatefulDevice]) -> Result<String, String> {
        let matching: Vec<&StatefulDevice> = devices
            .iter()
            .filter(|device| self.matches(device))
            .collect();
        match matching.as_slice() {
            [] => Err(format!("Destination {self} was not discovered")),
            [device] => Ok(device.device.fingerprint.clone()),
            devices => Err(format!(
                "Destination {self} is ambiguous ({} devices matched); use an IP address",
                devices.len()
            )),
        }
    }

    fn matches(&self, device: &StatefulDevice) -> bool {
        match self {
            Self::Alias(alias) => device.device.alias == *alias,
            Self::Ip(ip) => device.get_ranked_channels().into_iter().any(|channel| {
                channel.http().is_some_and(|http| {
                    http.host
                        .split('%')
                        .next()
                        .and_then(|host| host.parse::<IpAddr>().ok())
                        == Some(*ip)
                })
            }),
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

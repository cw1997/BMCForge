pub mod config;
pub mod targets;

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub manufacturer_id: u32,
    pub product_id: u16,
    pub model: String,
    pub firmware: String,
    pub fru: Vec<String>,
    /// Original Get Device ID bytes, including auxiliary firmware revision.
    #[serde(default)]
    pub device_id_raw: Vec<u8>,
    #[serde(default)]
    pub machine_types: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlValue {
    Percent(u8),
    Raw(u8),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sensor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub value: Option<f64>,
    pub unit: String,
    pub state: String,
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ControlState {
    #[default]
    Unknown,
    Requested {
        percent: u8,
        zone: u8,
    },
    RawRequested {
        value: u8,
        zone: u8,
    },
    Automatic,
    Uncertain,
}

pub fn redact(message: &str, secrets: &[String]) -> String {
    let mut result = message.to_owned();
    let mut secrets: Vec<_> = secrets.iter().filter(|s| !s.is_empty()).collect();
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for secret in secrets {
        result = result.replace(secret, "[REDACTED]");
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn redacts_overlapping_secrets() {
        assert_eq!(
            super::redact("secret123 secret", &["secret".into(), "secret123".into()]),
            "[REDACTED] [REDACTED]"
        );
    }
}

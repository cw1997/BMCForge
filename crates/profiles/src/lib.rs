use anyhow::{Context, Result, ensure};
use bmcforge_core::DeviceIdentity;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    HardwareVerified,
    Documented,
    Experimental,
    Unsupported,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Matcher {
    pub manufacturer_id: Option<u32>,
    pub product_ids: Vec<u16>,
    pub models: Vec<String>,
    pub excluded_models: Vec<String>,
    pub firmware_min: Option<String>,
    pub firmware_max: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DefaultCredential {
    pub username: String,
    pub password: Option<String>,
    pub source: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub id: u8,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub netfn: u8,
    pub command: u8,
    #[serde(default)]
    pub data: Vec<String>,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub expect: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FanControl {
    pub min_percent: u8,
    pub max_percent: u8,
    /// Raw value representing 100% duty: normally 100 or 255.
    pub scale: u8,
    pub zones: Vec<Zone>,
    pub manual: Vec<Step>,
    pub set: Vec<Step>,
    pub automatic: Vec<Step>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandExample {
    pub percent: u8,
    pub zone: u8,
    /// Each command: netfn, command, payload bytes.
    pub expected: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub vendor: String,
    pub description: String,
    pub verification: Verification,
    pub sources: Vec<String>,
    pub notes: String,
    pub matcher: Matcher,
    #[serde(default)]
    pub override_builtin: bool,
    #[serde(default)]
    pub default_credentials: Vec<DefaultCredential>,
    pub fan: Option<FanControl>,
    #[serde(default)]
    pub examples: Vec<CommandExample>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub netfn: u8,
    pub command: u8,
    pub data: Vec<u8>,
    pub delay_ms: u64,
    pub expect: Option<Vec<u8>>,
}

fn version(s: &str) -> Result<Vec<u32>> {
    s.split('.')
        .map(|part| {
            part.parse::<u32>()
                .context("firmware must contain numeric dot-separated components")
        })
        .collect()
}

fn version_cmp(a: &str, b: &str) -> Result<std::cmp::Ordering> {
    let mut a = version(a)?;
    let mut b = version(b)?;
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    Ok(a.cmp(&b))
}

impl Profile {
    pub fn parse(text: &str) -> Result<Self> {
        let p: Self = toml::from_str(text)?;
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported profile schema");
        ensure!(
            !self.id.is_empty()
                && self
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
            "invalid profile id"
        );
        ensure!(
            !self.sources.is_empty() && self.sources.iter().all(|s| s.starts_with("https://")),
            "profile needs HTTPS sources"
        );
        for v in [&self.matcher.firmware_min, &self.matcher.firmware_max]
            .into_iter()
            .flatten()
        {
            version(v)?;
        }
        if let (Some(min), Some(max)) = (&self.matcher.firmware_min, &self.matcher.firmware_max) {
            ensure!(
                version_cmp(min, max)? != std::cmp::Ordering::Greater,
                "firmware range reversed"
            );
        }
        for c in &self.default_credentials {
            ensure!(
                c.source.starts_with("https://") && !c.note.is_empty(),
                "credential needs source and applicability note"
            );
        }
        if let Some(fan) = &self.fan {
            ensure!(
                self.verification != Verification::Unsupported,
                "unsupported profile cannot contain fan commands"
            );
            ensure!(
                fan.min_percent <= fan.max_percent && fan.max_percent <= 100 && fan.scale > 0,
                "invalid duty range/scale"
            );
            ensure!(
                !fan.zones.is_empty() && !fan.set.is_empty() && !fan.automatic.is_empty(),
                "control needs zones, set and automatic sequences"
            );
            let mut zones = HashSet::new();
            for z in &fan.zones {
                ensure!(zones.insert(z.id), "duplicate zone");
            }
            for s in fan.manual.iter().chain(&fan.set).chain(&fan.automatic) {
                ensure!(
                    s.netfn < 64 && s.netfn % 2 == 0 && s.data.len() <= 128 && s.delay_ms <= 30_000,
                    "invalid IPMI step"
                );
                for token in &s.data {
                    Self::byte(token, fan.min_percent, fan.zones[0].id, fan.scale)?;
                }
            }
            ensure!(
                !self.examples.is_empty(),
                "control profile needs command examples"
            );
            for example in &self.examples {
                let commands = self.commands(example.percent, example.zone, true)?;
                let actual: Vec<Vec<u8>> = commands
                    .iter()
                    .map(|s| {
                        let mut v = vec![s.netfn, s.command];
                        v.extend(&s.data);
                        v
                    })
                    .collect();
                ensure!(
                    actual == example.expected,
                    "command example mismatch in {}",
                    self.id
                );
            }
        }
        Ok(())
    }
    fn byte(token: &str, percent: u8, zone: u8, scale: u8) -> Result<u8> {
        match token {
            "$duty" => Ok(((u16::from(percent) * u16::from(scale) + 50) / 100) as u8),
            "$percent" => Ok(percent),
            "$zone" => Ok(zone),
            _ => {
                if let Some(hex) = token.strip_prefix("0x") {
                    Ok(u8::from_str_radix(hex, 16)?)
                } else {
                    Ok(token.parse()?)
                }
            }
        }
    }
    pub fn commands(&self, percent: u8, zone: u8, include_manual: bool) -> Result<Vec<Command>> {
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        ensure!(
            (f.min_percent..=f.max_percent).contains(&percent),
            "duty must be {}..{}%",
            f.min_percent,
            f.max_percent
        );
        ensure!(f.zones.iter().any(|z| z.id == zone), "unknown fan zone");
        let steps: Vec<_> = if include_manual {
            f.manual.iter().chain(&f.set).collect()
        } else {
            f.set.iter().collect()
        };
        steps
            .into_iter()
            .map(|s| self.compile(s, percent, zone))
            .collect()
    }
    fn compile(&self, s: &Step, percent: u8, zone: u8) -> Result<Command> {
        let scale = self.fan.as_ref().context("monitoring-only profile")?.scale;
        Ok(Command {
            netfn: s.netfn,
            command: s.command,
            data: s
                .data
                .iter()
                .map(|t| Self::byte(t, percent, zone, scale))
                .collect::<Result<_>>()?,
            delay_ms: s.delay_ms,
            expect: s.expect.clone(),
        })
    }
    pub fn automatic(&self) -> Result<Vec<Command>> {
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        f.automatic
            .iter()
            .map(|s| self.compile(s, f.max_percent, f.zones[0].id))
            .collect()
    }
    pub fn matches(&self, d: &DeviceIdentity) -> bool {
        let m = &self.matcher;
        // A vendor ID by itself is never enough to infer OEM write support.
        if m.models.is_empty() && m.product_ids.is_empty() {
            return false;
        }
        self.compatible(d)
    }
    /// Enforce known hardware/firmware constraints even for explicit selection.
    pub fn compatible(&self, d: &DeviceIdentity) -> bool {
        let m = &self.matcher;
        if m.excluded_models
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&d.model))
        {
            return false;
        }
        if m.manufacturer_id.is_some_and(|id| id != d.manufacturer_id) {
            return false;
        }
        if !m.product_ids.is_empty() && !m.product_ids.contains(&d.product_id) {
            return false;
        }
        let model = d.model.to_ascii_lowercase();
        if !m.models.is_empty()
            && !m
                .models
                .iter()
                .any(|name| model == name.to_ascii_lowercase())
        {
            return false;
        }
        if let Some(min) = &m.firmware_min
            && !matches!(
                version_cmp(&d.firmware, min),
                Ok(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
            )
        {
            return false;
        }
        if let Some(max) = &m.firmware_max
            && !matches!(
                version_cmp(&d.firmware, max),
                Ok(std::cmp::Ordering::Equal | std::cmp::Ordering::Less)
            )
        {
            return false;
        }
        true
    }
    pub fn eligible(&self, experimental: bool) -> bool {
        self.fan.is_some()
            && self.verification != Verification::Unsupported
            && (self.verification != Verification::Experimental || experimental)
    }
}

pub struct Registry {
    pub profiles: BTreeMap<String, Profile>,
}
impl Registry {
    pub fn load(dir: Option<&Path>) -> Result<Self> {
        let builtins = [
            include_str!("../../../profiles/generic.toml"),
            include_str!("../../../profiles/dell-idrac6-8.toml"),
            include_str!("../../../profiles/dell-idrac9-legacy.toml"),
            include_str!("../../../profiles/supermicro-x10-x13.toml"),
            include_str!("../../../profiles/supermicro-x9.toml"),
            include_str!("../../../profiles/asrock-aspeed.toml"),
            include_str!("../../../profiles/asrock-ast2600.toml"),
        ];
        let mut profiles = BTreeMap::new();
        for text in builtins {
            let p = Profile::parse(text)?;
            profiles.insert(p.id.clone(), p);
        }
        if let Some(dir) = dir {
            let mut files: Vec<_> = std::fs::read_dir(dir)
                .with_context(|| format!("read profiles {}", dir.display()))?
                .collect::<std::io::Result<_>>()?;
            files.sort_by_key(|f| f.path());
            let mut external = HashSet::new();
            for entry in files {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                    continue;
                }
                let p = Profile::parse(&std::fs::read_to_string(&path)?)
                    .with_context(|| format!("profile {}", path.display()))?;
                ensure!(
                    external.insert(p.id.clone()),
                    "duplicate external profile {}",
                    p.id
                );
                ensure!(
                    !profiles.contains_key(&p.id) || p.override_builtin,
                    "{} needs override_builtin = true",
                    p.id
                );
                profiles.insert(p.id.clone(), p);
            }
        }
        Ok(Self { profiles })
    }
    pub fn candidates(&self, d: &DeviceIdentity) -> Vec<String> {
        self.profiles
            .values()
            .filter(|p| p.matches(d))
            .map(|p| p.id.clone())
            .collect()
    }
    pub fn schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Profile)).expect("schema serialization")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_validate_and_encode() -> Result<()> {
        let r = Registry::load(None)?;
        assert_eq!(r.profiles.len(), 7);
        let dell = &r.profiles["dell-idrac6-8"];
        assert_eq!(dell.commands(35, 255, true)?[1].data, [2, 255, 35]);
        assert!(dell.commands(101, 255, true).is_err());
        assert!(dell.commands(35, 0, true).is_err());
        let x9 = &r.profiles["supermicro-x9"];
        assert_eq!(x9.commands(50, 16, false)?[0].data.last(), Some(&128));
        assert!(!dell.eligible(false));
        assert!(dell.eligible(true));
        assert!(Registry::schema().get("definitions").is_some());
        Ok(())
    }
    #[test]
    fn firmware_and_matching_are_conservative() -> Result<()> {
        let r = Registry::load(None)?;
        let mut d = DeviceIdentity {
            manufacturer_id: 674,
            model: "PowerEdge R740".into(),
            firmware: "3.30.30".into(),
            ..Default::default()
        };
        assert_eq!(r.candidates(&d), ["dell-idrac9-legacy"]);
        d.firmware = "3.34.34".into();
        assert!(r.candidates(&d).is_empty());
        d.model = "PowerEdge R740xd unknown".into();
        assert!(r.candidates(&d).is_empty());
        assert_eq!(version_cmp("1.2", "1.2.0")?, std::cmp::Ordering::Equal);
        Ok(())
    }
    #[test]
    fn override_requires_explicit_flag() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let text = include_str!("../../../profiles/generic.toml");
        std::fs::write(dir.path().join("custom.toml"), text)?;
        assert!(Registry::load(Some(dir.path())).is_err());
        std::fs::write(
            dir.path().join("custom.toml"),
            format!("override_builtin = true\n{text}"),
        )?;
        Registry::load(Some(dir.path()))?;
        Ok(())
    }
}

use anyhow::{Context, Result, ensure};
pub mod models;
use bmcforge_core::{ControlValue, DeviceIdentity};
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
    pub manufacturer_ids: Vec<u32>,
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
    /// Expected response encoded with the same bounded parameter tokens as request data.
    #[serde(default)]
    pub expect_data: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FanControl {
    /// Optional exact percentage levels when intermediate duties are not documented.
    #[serde(default)]
    pub percent_values: Vec<u8>,
    /// Reviewed read-only protocol probes, run before the first takeover.
    #[serde(default)]
    pub probe: Vec<Step>,
    #[serde(default)]
    pub raw_values: Vec<u8>,
    #[serde(default)]
    pub restore_per_zone: bool,
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
    #[serde(default)]
    pub percent: u8,
    #[serde(default)]
    pub raw: Option<u8>,
    pub zone: u8,
    /// Each command: netfn, command, payload bytes.
    pub expected: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Trusted registry metadata for catalog aliases; never editable or serialized.
    #[serde(skip)]
    pub origin_id: Option<String>,
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
fn normalized_model(name: &str) -> String {
    let mut s: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    for prefix in ["dellinc", "dell", "lenovo", "ibm"] {
        if let Some(tail) = s.strip_prefix(prefix) {
            s = tail.to_owned();
            break;
        }
    }
    s
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
            ensure!(
                fan.probe
                    .iter()
                    .all(|p| p.expect.is_some() || p.expect_data.is_some()),
                "protocol probes require an expected response"
            );
            ensure!(
                fan.percent_values
                    .iter()
                    .all(|v| (fan.min_percent..=fan.max_percent).contains(v)),
                "percentage level outside adapter bounds"
            );
            ensure!(
                fan.raw_values.is_empty() || fan.percent_values.is_empty(),
                "raw and percentage levels cannot be mixed"
            );
            for s in fan
                .probe
                .iter()
                .chain(&fan.manual)
                .chain(&fan.set)
                .chain(&fan.automatic)
            {
                ensure!(
                    s.netfn < 64 && s.netfn % 2 == 0 && s.data.len() <= 128 && s.delay_ms <= 30_000,
                    "invalid IPMI step"
                );
                ensure!(
                    s.expect.is_none() || s.expect_data.is_none(),
                    "choose expect or expect_data"
                );
                ensure!(
                    s.expect_data.as_ref().is_none_or(|v| v.len() <= 128),
                    "expected response too long"
                );
                for token in s.data.iter().chain(s.expect_data.iter().flatten()) {
                    Self::byte(token, fan.min_percent, fan.zones[0].id, fan.scale)?;
                    ensure!(
                        fan.raw_values.is_empty() || (token != "$percent" && token != "$duty"),
                        "raw control cannot use percentage tokens"
                    );
                    ensure!(
                        !fan.raw_values.is_empty() || token != "$raw",
                        "percentage control cannot use raw token"
                    );
                }
            }
            ensure!(
                !self.examples.is_empty(),
                "control profile needs command examples"
            );
            for example in &self.examples {
                let value = example
                    .raw
                    .map(ControlValue::Raw)
                    .unwrap_or(ControlValue::Percent(example.percent));
                let commands = self.commands_value(value, example.zone, true)?;
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
            "$raw" => Ok(percent),
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
        self.commands_value(ControlValue::Percent(percent), zone, include_manual)
    }
    pub fn commands_value(
        &self,
        value: ControlValue,
        zone: u8,
        include_manual: bool,
    ) -> Result<Vec<Command>> {
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        let percent = match value {
            ControlValue::Percent(p) => {
                ensure!(
                    f.raw_values.is_empty(),
                    "profile requires a raw control value"
                );
                p
            }
            ControlValue::Raw(v) => {
                ensure!(f.raw_values.contains(&v), "raw value not in sourced levels");
                v
            }
        };
        ensure!(
            !f.raw_values.is_empty() || (f.min_percent..=f.max_percent).contains(&percent),
            "duty must be {}..{}%",
            f.min_percent,
            f.max_percent
        );
        ensure!(
            f.percent_values.is_empty() || f.percent_values.contains(&percent),
            "percentage is not a documented adapter level"
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
            expect: match &s.expect_data {
                Some(tokens) => Some(
                    tokens
                        .iter()
                        .map(|t| Self::byte(t, percent, zone, scale))
                        .collect::<Result<_>>()?,
                ),
                None => s.expect.clone(),
            },
        })
    }
    pub fn probe(&self, value: ControlValue, zone: u8) -> Result<Vec<Command>> {
        self.commands_value(value, zone, false)?;
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        let number = match value {
            ControlValue::Percent(p) | ControlValue::Raw(p) => p,
        };
        f.probe
            .iter()
            .map(|s| self.compile(s, number, zone))
            .collect()
    }
    pub fn automatic(&self) -> Result<Vec<Command>> {
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        if f.restore_per_zone {
            return f
                .zones
                .iter()
                .map(|z| self.automatic_zone(z.id))
                .collect::<Result<Vec<_>>>()
                .map(|v| v.into_iter().flatten().collect());
        }
        self.automatic_zone(f.zones[0].id)
    }
    pub fn automatic_zone(&self, zone: u8) -> Result<Vec<Command>> {
        let f = self.fan.as_ref().context("monitoring-only profile")?;
        ensure!(f.zones.iter().any(|z| z.id == zone), "unknown fan zone");
        f.automatic
            .iter()
            .map(|s| self.compile(s, f.max_percent, zone))
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
        if !m.manufacturer_ids.is_empty() && !m.manufacturer_ids.contains(&d.manufacturer_id) {
            return false;
        }
        if !m.product_ids.is_empty() && !m.product_ids.contains(&d.product_id) {
            return false;
        }
        let model = normalized_model(&d.model);
        let direct = m.models.iter().any(|name| model == normalized_model(name));
        if !m.models.is_empty() && (!direct || !d.machine_types.is_empty()) {
            let Ok(catalog) = models::Catalog::load(None) else {
                return false;
            };
            let candidates = catalog.candidates(d);
            if candidates.len() > 1 {
                return false;
            }
            if !direct
                && !candidates.first().is_some_and(|c| {
                    c.explicit
                        && catalog.models[&c.id].profile.as_deref()
                            == Some(self.origin_id.as_deref().unwrap_or(&self.id))
                })
            {
                return false;
            }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalProfile {
    pub origin_id: String,
    pub original: Profile,
    pub profile: Profile,
}

pub struct Registry {
    pub local: BTreeMap<String, LocalProfile>,
    pub origins: BTreeMap<String, std::path::PathBuf>,
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
            include_str!("../../../profiles/lenovo-x3650-m5-imm2.toml"),
            include_str!("../../../profiles/lenovo-x3550-m5-imm2.toml"),
            include_str!("../../../profiles/h3c-uis2000-g6.toml"),
            include_str!("../../../profiles/hpe-ilo-monitor.toml"),
            include_str!("../../../profiles/lenovo-xcc-monitor.toml"),
            include_str!("../../../profiles/ibm-lenovo-imm-monitor.toml"),
            include_str!("../../../profiles/inspur-monitor.toml"),
            include_str!("../../../profiles/h3c-hdm-monitor.toml"),
            include_str!("../../../profiles/huawei-ibmc-monitor.toml"),
            include_str!("../../../profiles/xfusion-ibmc-monitor.toml"),
            include_str!("../../../profiles/sugon-monitor.toml"),
            include_str!("../../../profiles/zte-monitor.toml"),
            include_str!("../../../profiles/greatwall-monitor.toml"),
        ];
        let mut profiles = BTreeMap::new();
        for text in builtins {
            let p = Profile::parse(text)?;
            profiles.insert(p.id.clone(), p);
        }
        let mut origins = BTreeMap::new();
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
                origins.insert(p.id.clone(), path.clone());
                profiles.insert(p.id.clone(), p);
            }
        }
        Ok(Self {
            profiles,
            local: BTreeMap::new(),
            origins,
        })
    }
    pub fn candidates(&self, d: &DeviceIdentity) -> Vec<String> {
        self.profiles
            .values()
            .filter(|p| {
                !self.local.values().any(|l| l.origin_id == p.id)
                    && (p.matches(d)
                        || self.local.get(&p.id).is_some_and(|l| {
                            self.profiles
                                .get(&l.origin_id)
                                .is_some_and(|original| original.matches(d))
                        }))
            })
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
    fn local_adapter_catalog_aliases_retain_identity_guards() -> Result<()> {
        let registry = Registry::load(None)?;
        let mut p = registry.profiles["lenovo-x3550-m5-imm2"].clone();
        let mut d = DeviceIdentity {
            manufacturer_id: 2,
            model: "5463AC1".into(),
            machine_types: vec!["5463".into()],
            ..Default::default()
        };
        assert!(p.compatible(&d));
        p.origin_id = Some(p.id.clone());
        p.id.push_str("-local-1");
        assert!(p.compatible(&d));
        d.machine_types = vec!["8871".into()];
        assert!(!p.compatible(&d));
        d.manufacturer_id = 674;
        assert!(!p.compatible(&d));
        assert!(toml::Value::try_from(&p)?.get("origin_id").is_none());
        Ok(())
    }
    #[test]
    fn builtins_validate_and_encode() -> Result<()> {
        let r = Registry::load(None)?;
        assert_eq!(r.profiles.len(), 20);
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
    #[test]
    fn raw_banks_are_not_percentages_and_cannot_bypass_identity() -> Result<()> {
        let r = Registry::load(None)?;
        let p = &r.profiles["lenovo-x3650-m5-imm2"];
        assert!(p.commands(64, 1, true).is_err());
        assert!(p.commands_value(ControlValue::Raw(63), 1, true).is_err());
        assert!(p.commands_value(ControlValue::Raw(64), 3, true).is_err());
        assert_eq!(
            p.commands_value(ControlValue::Raw(128), 2, true)?[0].data,
            [2, 128, 1]
        );
        assert_eq!(p.automatic_zone(2)?[0].data, [2, 18, 0]);
        let mut d = DeviceIdentity {
            manufacturer_id: 19046,
            model: "Lenovo System x3650 M5".into(),
            ..Default::default()
        };
        assert!(p.compatible(&d));
        d.manufacturer_id = 674;
        assert!(!p.compatible(&d));
        d.manufacturer_id = 19046;
        d.machine_types = vec!["5463".into()];
        assert!(!p.compatible(&d));
        let dell = &r.profiles["dell-idrac9-legacy"];
        d.manufacturer_id = 674;
        d.model = "Dell Inc. PowerEdge R740".into();
        d.machine_types.clear();
        assert!(!dell.compatible(&d));
        d.firmware = "3.30.30".into();
        assert!(dell.compatible(&d));
        d.firmware = "3.34".into();
        assert!(!dell.compatible(&d));
        Ok(())
    }
    #[test]
    fn x3550_m5_script_commands_and_identity_guards() -> Result<()> {
        let r = Registry::load(None)?;
        let p = &r.profiles["lenovo-x3550-m5-imm2"];
        assert!(!p.eligible(false));
        assert!(p.eligible(true));
        for zone in [1, 2] {
            let commands = p.commands_value(ControlValue::Raw(16), zone, true)?;
            assert_eq!((commands[0].netfn, commands[0].command), (58, 7));
            assert_eq!(commands[0].data, [zone, 16, 1]);
            assert_eq!(p.automatic_zone(zone)?[0].data, [zone, 18, 0]);
        }
        assert_eq!(p.automatic()?.len(), 2);
        assert!(
            p.commands_value(ControlValue::Percent(16), 1, true)
                .is_err()
        );
        assert!(p.commands_value(ControlValue::Raw(18), 1, true).is_err());
        assert!(p.commands_value(ControlValue::Raw(16), 3, true).is_err());
        for manufacturer_id in [2, 19046] {
            let mut d = DeviceIdentity {
                manufacturer_id,
                model: "IBM x3550 M5".into(),
                ..Default::default()
            };
            assert!(p.compatible(&d));
            d.machine_types = vec!["5463".into()];
            assert!(p.compatible(&d));
            d.machine_types = vec!["8871".into()];
            assert!(!p.compatible(&d));
            d.machine_types.clear();
            d.model = "System x3650 M5".into();
            assert!(!p.compatible(&d));
            d.model = "System x3550 M5".into();
            d.manufacturer_id = 674;
            assert!(!p.compatible(&d));
        }
        Ok(())
    }
    #[test]
    fn every_catalog_adapter_exists_and_monitoring_profiles_have_no_writes() -> Result<()> {
        let r = Registry::load(None)?;
        let catalog = models::Catalog::load(None)?;
        for m in catalog.models.values() {
            if let Some(id) = &m.profile {
                ensure!(
                    r.profiles.contains_key(id),
                    "model {}: unknown adapter {id}",
                    m.id
                );
            }
        }
        for id in [
            "hpe-ilo-monitor",
            "lenovo-xcc-monitor",
            "inspur-monitor",
            "h3c-hdm-monitor",
            "huawei-ibmc-monitor",
            "xfusion-ibmc-monitor",
            "sugon-monitor",
            "zte-monitor",
            "greatwall-monitor",
        ] {
            let p = &r.profiles[id];
            assert!(!p.eligible(true));
            assert!(p.commands(50, 0, true).is_err());
            assert!(p.automatic().is_err());
        }
        let p = &r.profiles["h3c-uis2000-g6"];
        assert!(p.commands(21, 0, true).is_err());
        assert!(p.commands_value(ControlValue::Raw(51), 0, true).is_err());
        assert_eq!(
            p.probe(ControlValue::Percent(20), 0)?[0].data,
            [170, 229, 0, 1]
        );
        for (percent, duty) in [(20, 51), (40, 102), (60, 153), (80, 204), (100, 255)] {
            let commands = p.commands(percent, 0, true)?;
            assert_eq!(commands[1].expect, Some(vec![170, 229, 0, duty]));
            assert_eq!(commands[1].data.last(), Some(&duty));
        }
        let d = DeviceIdentity {
            manufacturer_id: 25506,
            model: "H3C UniServer R4900 G6".into(),
            ..Default::default()
        };
        assert!(!p.compatible(&d));
        Ok(())
    }
}

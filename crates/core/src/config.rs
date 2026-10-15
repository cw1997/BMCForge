use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

impl Default for Credentials {
    fn default() -> Self {
        Self {
            username: "ADMIN".into(),
            password: String::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub credentials: Credentials,
    pub profile: Option<String>,
    /// User-confirmed catalog ID; never replaces raw FRU evidence.
    pub model_id: Option<String>,
    pub identity_evidence: Option<crate::DeviceIdentity>,
    pub identification_error: Option<String>,
    pub tags: Vec<String>,
    pub timeout_ms: u64,
    pub retries: u32,
    pub poll_seconds: u64,
    pub privilege: u8,
    pub kg: Option<String>,
    pub allow_experimental: bool,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("id", &self.id)
            .field("host", &self.host)
            .field("credentials", &self.credentials)
            .field("kg", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl Default for Server {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 623,
            credentials: Credentials::default(),
            profile: None,
            model_id: None,
            identity_evidence: None,
            identification_error: None,
            tags: vec![],
            timeout_ms: 2000,
            retries: 1,
            poll_seconds: 5,
            privilege: 4,
            kg: None,
            allow_experimental: false,
        }
    }
}

impl Server {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_empty() && !self.host.is_empty(),
            "server id and host cannot be empty"
        );
        ensure!(
            self.port > 0 && self.timeout_ms > 0 && self.poll_seconds > 0,
            "port and intervals must be positive"
        );
        ensure!((1..=5).contains(&self.privilege), "invalid privilege level");
        ensure!(
            self.credentials.username.len() <= 16,
            "IPMI username exceeds 16 bytes"
        );
        ensure!(
            self.credentials.password.len() <= 20,
            "IPMI password exceeds 20 bytes"
        );
        ensure!(
            self.kg.as_ref().is_none_or(|key| key.len() <= 20),
            "IPMI Kg exceeds 20 bytes"
        );
        ensure!(self.retries <= 10, "retries must be <= 10");
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub mouse: bool,
    pub concurrency: usize,
    pub scan_concurrency: usize,
    pub scan_port: u16,
    pub scan_timeout_ms: u64,
    pub scan_retries: u32,
    pub scan_confirm_above: usize,
    pub scan_rate_per_second: u32,
    pub history_capacity: usize,
    pub temperature_unit: String,
    pub sensor_filter: String,
    pub keys: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            theme: "dark".into(),
            mouse: true,
            concurrency: 8,
            scan_concurrency: 32,
            scan_port: 623,
            scan_timeout_ms: 2000,
            scan_retries: 1,
            scan_confirm_above: 4096,
            scan_rate_per_second: 32,
            history_capacity: 300,
            temperature_unit: "C".into(),
            sensor_filter: String::new(),
            keys: BTreeMap::from([
                ("quit".into(), "q".into()),
                ("import".into(), "i".into()),
                ("discover".into(), "d".into()),
                ("manual".into(), "m".into()),
                ("automatic".into(), "a".into()),
                ("edit".into(), "e".into()),
                ("select".into(), " ".into()),
                ("settings".into(), ",".into()),
                ("profiles".into(), "p".into()),
                ("logs".into(), "l".into()),
                ("search".into(), "/".into()),
                ("export".into(), "x".into()),
            ]),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub settings: Settings,
    pub servers: Vec<Server>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            settings: Settings::default(),
            servers: vec![],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported config version {}",
            self.version
        );
        let s = &self.settings;
        ensure!(
            s.concurrency <= 1024 && s.scan_concurrency <= 1024,
            "concurrency must be <= 1024"
        );
        ensure!(
            s.concurrency > 0 && s.scan_concurrency > 0 && s.scan_rate_per_second > 0,
            "concurrency and scan rate must be positive"
        );
        ensure!(
            s.history_capacity > 0
                && s.scan_timeout_ms > 0
                && s.scan_confirm_above > 0
                && s.scan_port > 0,
            "capacities and timeout must be positive"
        );
        ensure!(s.scan_retries <= 10, "scan retries must be <= 10");
        ensure!(
            !s.language.is_empty()
                && s.language.len() <= 64
                && s.language
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "language must be a language tag"
        );
        ensure!(
            ["dark", "light"].contains(&s.theme.as_str()),
            "theme must be dark or light"
        );
        ensure!(
            ["C", "F"].contains(&s.temperature_unit.as_str()),
            "temperature_unit must be C or F"
        );
        let mut keys = std::collections::HashSet::new();
        for key in s.keys.values() {
            ensure!(
                key.chars().count() == 1 && !"12345".contains(key),
                "shortcut must be one character other than 1..5"
            );
            ensure!(keys.insert(key), "duplicate shortcut: {key}");
        }
        let mut ids = std::collections::HashSet::new();
        let mut endpoints = std::collections::HashSet::new();
        for server in &self.servers {
            server.validate()?;
            ensure!(ids.insert(&server.id), "duplicate server id {}", server.id);
            ensure!(
                endpoints.insert(super::targets::canonical_endpoint(
                    &server.host,
                    server.port
                )),
                "duplicate endpoint {}",
                server.endpoint()
            );
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let contents =
            fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        cfg.validate()
            .with_context(|| format!("validate {}", path.display()))?;
        Ok(cfg)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temporary.write_all(text.as_bytes())?;
        temporary.as_file().sync_all()?;
        if path.exists() {
            // Never replace a damaged or future-version configuration.
            Self::load(path).context("existing configuration is invalid; refusing overwrite")?;
            let backup = PathBuf::from(format!("{}.bak", path.display()));
            let mut backup_temp = tempfile::NamedTempFile::new_in(parent)?;
            std::io::copy(&mut fs::File::open(path)?, &mut backup_temp)?;
            backup_temp.as_file().sync_all()?;
            backup_temp.persist(&backup)?;
        }
        temporary.persist(path)?;
        Ok(())
    }
    pub fn secrets(&self) -> Vec<String> {
        self.servers
            .iter()
            .flat_map(|s| {
                [
                    s.credentials.password.clone(),
                    s.kg.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }
}

pub fn default_path() -> Result<PathBuf> {
    directories::ProjectDirs::from("org", "BMCForge", "BMCForge")
        .map(|d| d.config_dir().join("config.toml"))
        .ok_or_else(|| {
            anyhow::anyhow!("cannot determine user configuration directory; use --config")
        })
}

pub fn parse_server(text: &str) -> Result<Server> {
    let server: Server = toml::from_str(text)?;
    server.validate()?;
    Ok(server)
}

pub fn require_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("not a file: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_backup_and_corrupt_protection() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.servers.push(Server {
            id: "s1".into(),
            host: "bmc.example".into(),
            credentials: Credentials {
                username: "u".into(),
                password: "秘密".into(),
            },
            ..Default::default()
        });
        cfg.settings.language = "zh-CN".into();
        cfg.save(&path)?;
        assert_eq!(Config::load(&path)?.servers[0].credentials.password, "秘密");
        cfg.settings.language = "en".into();
        cfg.save(&path)?;
        assert_eq!(
            Config::load(&path.with_extension("toml.bak"))?
                .settings
                .language,
            "zh-CN"
        );
        fs::write(&path, "broken = [")?;
        assert!(cfg.save(&path).is_err());
        assert_eq!(fs::read_to_string(&path)?, "broken = [");
        assert!(!format!("{:?}", cfg.servers[0].credentials).contains("秘密"));
        Ok(())
    }
    #[test]
    fn rejects_duplicate_hosts_and_keys() {
        let mut c = Config {
            servers: vec![
                Server {
                    id: "a".into(),
                    host: "BMC.".into(),
                    ..Default::default()
                },
                Server {
                    id: "b".into(),
                    host: "bmc".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(c.validate().is_err());
        c.servers.clear();
        c.settings.keys.insert("quit".into(), "i".into());
        assert!(c.validate().is_err());
    }
    #[test]
    fn new_defaults_and_saved_preferences_are_distinct() -> Result<()> {
        let new: Config = toml::from_str("version = 1")?;
        assert_eq!(new.settings.language, "en");
        assert!(new.settings.mouse);
        let old: Config =
            toml::from_str("version = 1\n[settings]\nlanguage = 'zh-CN'\nmouse = false")?;
        old.validate()?;
        assert_eq!(old.settings.language, "zh-CN");
        assert!(!old.settings.mouse);
        let reloaded: Config = toml::from_str(&toml::to_string(&old)?)?;
        assert_eq!(reloaded.settings.language, "zh-CN");
        assert!(!reloaded.settings.mouse);
        Ok(())
    }
}

use crate::{
    editor::Editor,
    runtime::{Command, Message, Snapshot},
};
use anyhow::{Context, Result, ensure};
use bmcforge_core::{
    config::{Config, Credentials, Server, Settings},
    redact, targets,
};
use bmcforge_ipmi::discovery::{DiscoveryResult, ProbeStatus};
use bmcforge_profiles::{Profile, Registry};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
};

pub enum Action {
    Add(Vec<Server>),
    Configure(Server),
    Send(String, Command),
    Scan(Vec<String>, u16),
    CancelScan,
    Authenticate(Vec<Server>),
    Forget(String),
    Quit,
}
#[derive(Clone)]
pub enum EditKind {
    Targets(bool),
    Credentials(Vec<String>, bool),
    Server(String),
    Settings,
    Search,
    Manual(Vec<String>),
}
pub enum Modal {
    Import {
        servers: Vec<Server>,
        authenticate: bool,
        duplicates: usize,
    },
    Editor {
        title: String,
        kind: EditKind,
        editor: Editor,
        error: String,
    },
    Preview {
        hosts: Vec<String>,
        discovery: bool,
        duplicates: usize,
        errors: Vec<String>,
    },
    Confirm {
        title: String,
        text: String,
        control: Vec<(String, Profile)>,
        manual: Option<(u8, u8)>,
    },
    Info(String),
    Remove {
        id: String,
        name: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportForm {
    username: String,
    password: String,
    port: u16,
    tags: Vec<String>,
    profile: Option<String>,
    allow_experimental: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualForm {
    percent: u8,
    zone: u8,
}

pub struct App {
    pub config: Config,
    pub path: PathBuf,
    pub registry: Registry,
    pub snapshots: BTreeMap<String, Snapshot>,
    pub history: BTreeMap<String, VecDeque<u64>>,
    pub selected: BTreeSet<String>,
    pub cursor: usize,
    pub page: usize,
    pub profile_cursor: usize,
    pub scan_cursor: usize,
    pub discovery: Vec<DiscoveryResult>,
    pub discovery_selected: BTreeSet<String>,
    pub scan_total: usize,
    pub scan_generation: u64,
    pub scanning: bool,
    pub candidates_only: bool,
    pub search: String,
    pub logs: VecDeque<String>,
    pub modal: Option<Modal>,
    pub modal_scroll: u16,
    pub status: String,
    pub demo: bool,
    pub detail_scroll: u16,
    pub sensor_scroll: usize,
    pub log_scroll: u16,
    sequence: u64,
}
impl App {
    pub fn new(config: Config, path: PathBuf, registry: Registry, demo: bool) -> Self {
        Self {
            config,
            path,
            registry,
            snapshots: BTreeMap::new(),
            history: BTreeMap::new(),
            selected: BTreeSet::new(),
            cursor: 0,
            page: 0,
            profile_cursor: 0,
            scan_cursor: 0,
            discovery: vec![],
            discovery_selected: BTreeSet::new(),
            scan_total: 0,
            scan_generation: 0,
            scanning: false,
            candidates_only: true,
            search: String::new(),
            logs: VecDeque::new(),
            modal: None,
            modal_scroll: 0,
            status: "Ready".into(),
            demo,
            detail_scroll: 0,
            sensor_scroll: 0,
            log_scroll: 0,
            sequence: 0,
        }
    }
    pub fn tr<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        if self.config.settings.language == "en" {
            en
        } else {
            zh
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        let query = self.search.to_lowercase();
        let mut items: Vec<_> = self
            .config
            .servers
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                format!("{} {} {}", s.name, s.host, s.tags.join(" "))
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        items.sort_by_key(|i| {
            (
                self.config.servers[*i]
                    .tags
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                self.config.servers[*i].name.clone(),
            )
        });
        items
    }
    pub fn current(&self) -> Option<&Server> {
        self.visible()
            .get(self.cursor)
            .and_then(|i| self.config.servers.get(*i))
    }
    pub fn targets(&self) -> Vec<String> {
        if self.selected.is_empty() {
            self.current()
                .map(|s| vec![s.id.clone()])
                .unwrap_or_default()
        } else {
            self.selected.iter().cloned().collect()
        }
    }
    pub fn scan_visible(&self) -> Vec<usize> {
        self.discovery
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (!self.candidates_only || r.status == ProbeStatus::IpmiCandidate)
                    && r.host.contains(&self.search)
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn log(&mut self, message: impl AsRef<str>) {
        let message = redact(message.as_ref(), &self.config.secrets());
        self.status.clone_from(&message);
        self.logs.push_back(message);
        while self.logs.len() > self.config.settings.history_capacity {
            self.logs.pop_front();
        }
    }
    fn persist(&mut self, updated: Config) -> Result<()> {
        updated.validate()?;
        if !self.demo {
            updated.save(&self.path)?;
        }
        self.config = updated;
        Ok(())
    }
    pub fn add_servers(&mut self, servers: Vec<Server>) -> Result<Vec<Server>> {
        let mut cfg = self.config.clone();
        let mut added = vec![];
        for server in servers {
            let endpoint = targets::canonical_endpoint(&server.host, server.port);
            if cfg
                .servers
                .iter()
                .any(|s| targets::canonical_endpoint(&s.host, s.port) == endpoint)
            {
                continue;
            }
            cfg.servers.push(server.clone());
            added.push(server);
        }
        self.persist(cfg)?;
        self.log(format!("Added {} servers", added.len()));
        Ok(added)
    }
    pub fn message(&mut self, message: Message) -> Vec<Action> {
        match message {
            Message::Snapshot(id, mut snapshot) => {
                if !self.config.servers.iter().any(|s| s.id == id) {
                    return vec![];
                }
                snapshot.status = redact(&snapshot.status, &self.config.secrets());
                if let Some(rpm) = snapshot
                    .sensors
                    .iter()
                    .filter(|s| s.unit == "RPM")
                    .filter_map(|s| s.value)
                    .reduce(f64::max)
                {
                    let samples = self.history.entry(id.clone()).or_default();
                    samples.push_back(rpm.max(0.0) as u64);
                    while samples.len() > self.config.settings.history_capacity {
                        samples.pop_front();
                    }
                }
                self.snapshots.insert(id, snapshot);
            }
            Message::Log(log) => self.log(log),
            Message::Discovered(generation, result) => {
                if generation == self.scan_generation {
                    self.discovery.push(result);
                }
            }
            Message::ScanFinished(generation) if generation == self.scan_generation => {
                self.scanning = false;
                self.log("Discovery finished / canceled. No response means unknown, not absent.");
            }
            Message::ScanFinished(_) => {}
            Message::Configured(server, error) => {
                if let Some(error) = error {
                    self.log(format!("configuration unchanged: {error}"));
                } else {
                    let mut cfg = self.config.clone();
                    if let Some(old) = cfg.servers.iter_mut().find(|s| s.id == server.id) {
                        if *old == server {
                            self.log("Running configuration restored");
                            return vec![];
                        }
                        let rollback = old.clone();
                        *old = server;
                        if let Err(error) = self.persist(cfg) {
                            self.log(format!(
                                "save failed, rolling back running configuration: {error:#}"
                            ));
                            return vec![Action::Configure(rollback)];
                        }
                        self.log("Server configuration saved");
                    }
                }
            }
            Message::Removed(id, error) => {
                if let Some(error) = error {
                    self.log(format!(
                        "remove canceled; automatic restore failed: {error}"
                    ));
                } else {
                    let mut cfg = self.config.clone();
                    let old = cfg.servers.iter().find(|s| s.id == id).cloned();
                    cfg.servers.retain(|s| s.id != id);
                    if let Err(error) = self.persist(cfg) {
                        self.log(format!("remove could not be saved: {error:#}"));
                        let mut actions = vec![Action::Forget(id)];
                        if let Some(server) = old {
                            actions.push(Action::Add(vec![server]));
                        }
                        return actions;
                    }
                    self.selected.remove(&id);
                    self.snapshots.remove(&id);
                    self.history.remove(&id);
                    self.cursor = self.cursor.min(self.visible().len().saturating_sub(1));
                    self.log("Server removed after automatic restore");
                    return vec![Action::Forget(id)];
                }
            }
            Message::Authenticated(server, error) => {
                if let Some(error) = error {
                    self.log(format!(
                        "{} authentication / read failed: {}",
                        server.host,
                        redact(
                            &error,
                            &[
                                server.credentials.password.clone(),
                                server.kg.clone().unwrap_or_default()
                            ]
                        )
                    ));
                } else {
                    match self.add_servers(vec![server]) {
                        Ok(added) => return vec![Action::Add(added)],
                        Err(error) => self.log(format!("save failed: {error:#}")),
                    }
                }
            }
        }
        vec![]
    }
    fn edit(&mut self, title: impl Into<String>, kind: EditKind, text: &str) {
        self.modal = Some(Modal::Editor {
            title: title.into(),
            kind,
            editor: Editor::new(text),
            error: String::new(),
        });
    }
    fn shortcut(&self, key: KeyEvent, action: &str) -> bool {
        key.modifiers.is_empty()
            && matches!(key.code, KeyCode::Char(c) if self.config.settings.keys.get(action).is_some_and(|s| s == &c.to_string()))
    }
    pub fn input(&mut self, key: KeyEvent) -> Vec<Action> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return vec![];
        }
        if self.modal.is_some() {
            return self.modal_input(key);
        }
        let result = self.normal_input(key);
        match result {
            Ok(actions) => actions,
            Err(error) => {
                self.log(format!("{error:#}"));
                vec![]
            }
        }
    }
    fn normal_input(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        if self.shortcut(key, "quit") {
            return Ok(vec![Action::Quit]);
        }
        if self.shortcut(key, "search") {
            self.edit(
                self.tr(
                    "搜索服务器 / 标签 / 地址",
                    "Search servers / tags / addresses",
                ),
                EditKind::Search,
                &self.search.clone(),
            );
            return Ok(vec![]);
        }
        if self.shortcut(key, "import") {
            if self.page == 2 {
                let hosts: Vec<_> = self
                    .discovery
                    .iter()
                    .filter(|r| {
                        r.status == ProbeStatus::IpmiCandidate
                            && (self.discovery_selected.is_empty()
                                || self.discovery_selected.contains(&r.host))
                    })
                    .map(|r| r.host.clone())
                    .collect();
                ensure!(!hosts.is_empty(), "no IPMI candidates selected");
                self.credentials(hosts, true)?;
            } else {
                self.edit(
                    self.tr(
                        "批量导入：每行 IP、主机名、CIDR 或范围",
                        "Import: IP / hostname / CIDR / range, one per line",
                    ),
                    EditKind::Targets(false),
                    "",
                );
            }
            return Ok(vec![]);
        }
        if self.shortcut(key, "discover") {
            if self.scanning {
                self.scanning = false;
                return Ok(vec![Action::CancelScan]);
            }
            self.edit(
                self.tr(
                    "发现：每行 IPv4、CIDR 或范围",
                    "Discover: IPv4 / CIDR / range, one per line",
                ),
                EditKind::Targets(true),
                if self.demo { "192.0.2.8/30" } else { "" },
            );
            return Ok(vec![]);
        }
        if self.shortcut(key, "settings") {
            self.edit(
                self.tr("设置：TOML", "Settings: TOML"),
                EditKind::Settings,
                &toml::to_string_pretty(&self.config.settings)?,
            );
            return Ok(vec![]);
        }
        if self.shortcut(key, "profiles") {
            self.page = 3;
            return Ok(vec![]);
        }
        if self.shortcut(key, "logs") {
            self.page = 4;
            return Ok(vec![]);
        }
        if self.shortcut(key, "export") {
            self.export()?;
            return Ok(vec![]);
        }
        if self.shortcut(key, "edit") {
            let server = self.current().context("select a server first")?;
            ensure!(
                !self.snapshots.get(&server.id).is_some_and(|s| s.taken_over),
                "restore automatic mode before editing this server"
            );
            self.edit(
                self.tr(
                    "服务器配置（含明文密码）",
                    "Server settings (contains plaintext password)",
                ),
                EditKind::Server(server.id.clone()),
                &toml::to_string_pretty(server)?,
            );
            return Ok(vec![]);
        }
        if self.shortcut(key, "manual") {
            let ids = self.targets();
            ensure!(!ids.is_empty(), "select a server first");
            let profile = self.profile(&ids[0])?;
            let zone = profile.fan.as_ref().context("monitoring only")?.zones[0].id;
            self.edit(
                self.tr(
                    "固定占空比：百分比及区域 ID",
                    "Fixed duty: percentage and zone ID",
                ),
                EditKind::Manual(ids),
                &toml::to_string_pretty(&ManualForm { percent: 50, zone })?,
            );
            return Ok(vec![]);
        }
        if self.shortcut(key, "automatic") {
            self.confirm_control(self.targets(), None)?;
            return Ok(vec![]);
        }
        if self.shortcut(key, "select") {
            if self.page == 2 {
                if let Some(index) = self.scan_visible().get(self.scan_cursor) {
                    let host = self.discovery[*index].host.clone();
                    if !self.discovery_selected.remove(&host) {
                        self.discovery_selected.insert(host);
                    }
                }
            } else if let Some(server) = self.current() {
                let id = server.id.clone();
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
            }
            return Ok(vec![]);
        }
        match key.code {
            KeyCode::Delete => if let Some(server)=self.current() { self.modal=Some(Modal::Remove { id:server.id.clone(),name:server.name.clone() }); },
            KeyCode::F(1) => self.modal=Some(Modal::Info("1..5/Tab: pages | Up/Down: navigate | Space: select | i: import | d: discover/cancel | e: server settings | m/a: manual/auto | p: profiles | u on profiles: factory credentials | ,: settings | x: redacted diagnostics | Del: remove after restore | F2/Ctrl+Enter: save editor | q/Ctrl+C: restore & exit. Keys are customizable in settings.".into())),
            KeyCode::Char('1'..='5') => {
                if let KeyCode::Char(c) = key.code {
                    self.page = c as usize - '1' as usize;
                }
            }
            KeyCode::Tab => self.page = (self.page + 1) % 5,
            KeyCode::BackTab => self.page = (self.page + 4) % 5,
            KeyCode::Esc => {
                self.search.clear();
                self.selected.clear();
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(true),
            KeyCode::PageDown => {
                if self.page == 4 {
                    self.log_scroll = self.log_scroll.saturating_add(10);
                } else {
                    if self.page == 1 { self.sensor_scroll = self.sensor_scroll.saturating_add(10); }
                    else { self.detail_scroll = self.detail_scroll.saturating_add(10); }
                }
            }
            KeyCode::PageUp => {
                if self.page == 4 {
                    self.log_scroll = self.log_scroll.saturating_sub(10);
                } else {
                    if self.page == 1 { self.sensor_scroll = self.sensor_scroll.saturating_sub(10); }
                    else { self.detail_scroll = self.detail_scroll.saturating_sub(10); }
                }
            }
            KeyCode::Enter if self.page == 0 => self.page = 1,
            KeyCode::Enter if self.page == 3 => self.assign_profile()?,
            KeyCode::Char('u') if self.page == 3 => self.default_credential()?,
            KeyCode::Char('f') if self.page == 2 => {
                self.candidates_only = !self.candidates_only;
                self.scan_cursor = 0;
            }
            _ => {}
        }
        Ok(vec![])
    }
    fn move_cursor(&mut self, down: bool) {
        let (cursor, count) = if self.page == 3 {
            (&mut self.profile_cursor, self.registry.profiles.len())
        } else if self.page == 2 {
            let count = self.scan_visible().len();
            (&mut self.scan_cursor, count)
        } else {
            let count = self.visible().len();
            (&mut self.cursor, count)
        };
        *cursor = if down {
            (*cursor + 1).min(count.saturating_sub(1))
        } else {
            cursor.saturating_sub(1)
        };
        self.detail_scroll = 0;
        self.sensor_scroll = 0;
    }
    fn profile(&self, id: &str) -> Result<Profile> {
        let server = self
            .config
            .servers
            .iter()
            .find(|s| s.id == id)
            .context("server missing")?;
        let profile = if let Some(id) = &server.profile {
            id.clone()
        } else {
            let snapshot = self
                .snapshots
                .get(&server.id)
                .context("wait for identification or choose a profile")?;
            let choices = self.registry.candidates(&snapshot.identity);
            ensure!(
                choices.len() == 1,
                "choose a profile explicitly ({} candidates)",
                choices.len()
            );
            choices[0].clone()
        };
        let profile = self
            .registry
            .profiles
            .get(&profile)
            .context("unknown profile")?
            .clone();
        ensure!(
            profile.eligible(server.allow_experimental),
            "{}: control unavailable; experimental profiles require allow_experimental = true in server settings",
            profile.id
        );
        Ok(profile)
    }
    fn assign_profile(&mut self) -> Result<()> {
        let id = self
            .registry
            .profiles
            .keys()
            .nth(self.profile_cursor)
            .context("select profile")?
            .clone();
        let current = self
            .current()
            .context("select server on page 1 first")?
            .id
            .clone();
        ensure!(
            !self.snapshots.get(&current).is_some_and(|s| s.taken_over),
            "restore automatic mode first"
        );
        let mut cfg = self.config.clone();
        cfg.servers
            .iter_mut()
            .find(|s| s.id == current)
            .unwrap()
            .profile = Some(id.clone());
        self.persist(cfg)?;
        self.log(format!("Profile {id} selected; experimental writes still require explicit enablement in server settings"));
        Ok(())
    }
    fn default_credential(&mut self) -> Result<()> {
        let p = self
            .registry
            .profiles
            .values()
            .nth(self.profile_cursor)
            .context("select profile")?;
        let mut server = self.current().context("select server first")?.clone();
        let credential = p
            .default_credentials
            .first()
            .context("no universal default credentials; use device label or your own account")?;
        server.credentials = Credentials {
            username: credential.username.clone(),
            password: credential.password.clone().unwrap_or_default(),
        };
        let title = format!("{} — {}", p.id, credential.note);
        self.edit(
            title,
            EditKind::Server(server.id.clone()),
            &toml::to_string_pretty(&server)?,
        );
        Ok(())
    }
    fn confirm_control(&mut self, ids: Vec<String>, manual: Option<(u8, u8)>) -> Result<()> {
        ensure!(!ids.is_empty(), "no server selected");
        let mut profiles = vec![];
        let mut lines = vec![];
        for id in ids {
            let profile = self.profile(&id)?;
            if let Some((percent, zone)) = manual {
                profile.commands(percent, zone, true)?;
            }
            let server = self.config.servers.iter().find(|s| s.id == id).unwrap();
            lines.push(format!(
                "{} ({}) — {}",
                server.name,
                server.endpoint(),
                profile.id
            ));
            profiles.push((id, profile));
        }
        let operation = manual
            .map(|(p, z)| format!("Request {p}% duty, zone {z}; actual duty unknown"))
            .unwrap_or_else(|| "Restore BMC automatic fan mode".into());
        self.modal = Some(Modal::Confirm {
            title: operation,
            text: lines.join("\n"),
            control: profiles,
            manual,
        });
        Ok(())
    }
    fn credentials(&mut self, hosts: Vec<String>, authenticated: bool) -> Result<()> {
        let form = ImportForm {
            username: if self.demo {
                "demo".into()
            } else {
                "ADMIN".into()
            },
            password: if self.demo {
                "demo".into()
            } else {
                String::new()
            },
            port: if authenticated {
                self.config.settings.scan_port
            } else {
                623
            },
            tags: vec![],
            profile: None,
            allow_experimental: false,
        };
        self.edit(
            self.tr(
                "导入凭据（明文）；profile 可指定适配 ID",
                "Import credentials (plaintext); optional profile ID",
            ),
            EditKind::Credentials(hosts, authenticated),
            &toml::to_string_pretty(&form)?,
        );
        Ok(())
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(Modal::Editor { editor, .. }) = &mut self.modal {
            editor.paste(text);
        }
    }
    fn modal_input(&mut self, key: KeyEvent) -> Vec<Action> {
        if key.code == KeyCode::Esc {
            self.modal = None;
            self.modal_scroll = 0;
            return vec![];
        }
        if !matches!(self.modal, Some(Modal::Editor { .. })) {
            if key.code == KeyCode::PageDown {
                self.modal_scroll = self.modal_scroll.saturating_add(8);
                return vec![];
            }
            if key.code == KeyCode::PageUp {
                self.modal_scroll = self.modal_scroll.saturating_sub(8);
                return vec![];
            }
        }
        let Some(modal) = self.modal.take() else {
            return vec![];
        };
        if key.code == KeyCode::Enter {
            self.modal_scroll = 0;
        }
        match modal {
            Modal::Editor {
                title,
                kind,
                mut editor,
                ..
            } => {
                if key.code == KeyCode::F(2)
                    || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    match self.submit(kind.clone(), &editor.text()) {
                        Ok(actions) => actions,
                        Err(error) => {
                            self.modal = Some(Modal::Editor {
                                title,
                                kind,
                                editor,
                                error: format!("{error:#}"),
                            });
                            vec![]
                        }
                    }
                } else {
                    editor.input(key);
                    self.modal = Some(Modal::Editor {
                        title,
                        kind,
                        editor,
                        error: String::new(),
                    });
                    vec![]
                }
            }
            Modal::Preview {
                hosts,
                discovery,
                duplicates,
                errors,
            } => {
                if key.code != KeyCode::Enter {
                    self.modal = Some(Modal::Preview {
                        hosts,
                        discovery,
                        duplicates,
                        errors,
                    });
                    return vec![];
                }
                if hosts.is_empty() || !errors.is_empty() {
                    self.modal = Some(Modal::Info("Fix invalid input lines before importing. Esc to close and enter the targets again.".into()));
                    return vec![];
                }
                if discovery {
                    self.discovery.clear();
                    self.discovery_selected.clear();
                    self.scan_cursor = 0;
                    self.scan_total = hosts.len();
                    self.scanning = true;
                    self.page = 2;
                    vec![Action::Scan(hosts, self.config.settings.scan_port)]
                } else {
                    if let Err(e) = self.credentials(hosts, false) {
                        self.log(format!("{e:#}"));
                    }
                    vec![]
                }
            }
            Modal::Confirm {
                title,
                text,
                control,
                manual,
            } => {
                if key.code == KeyCode::Enter {
                    control
                        .into_iter()
                        .map(|(id, p)| {
                            Action::Send(
                                id,
                                if let Some((percent, zone)) = manual {
                                    Command::Manual(Box::new(p), percent, zone)
                                } else {
                                    Command::Automatic(Some(Box::new(p)))
                                },
                            )
                        })
                        .collect()
                } else {
                    self.modal = Some(Modal::Confirm {
                        title,
                        text,
                        control,
                        manual,
                    });
                    vec![]
                }
            }
            Modal::Info(text) => {
                if key.code != KeyCode::Enter {
                    self.modal = Some(Modal::Info(text));
                }
                vec![]
            }
            Modal::Remove { id, name } => {
                if key.code == KeyCode::Enter {
                    vec![Action::Send(id, Command::Remove)]
                } else {
                    self.modal = Some(Modal::Remove { id, name });
                    vec![]
                }
            }
            Modal::Import {
                servers,
                authenticate,
                duplicates,
            } => {
                if key.code == KeyCode::Enter {
                    if authenticate {
                        vec![Action::Authenticate(servers)]
                    } else {
                        match self.add_servers(servers) {
                            Ok(added) => vec![Action::Add(added)],
                            Err(error) => {
                                self.log(format!("import save failed: {error:#}"));
                                vec![]
                            }
                        }
                    }
                } else {
                    self.modal = Some(Modal::Import {
                        servers,
                        authenticate,
                        duplicates,
                    });
                    vec![]
                }
            }
        }
    }
    fn submit(&mut self, kind: EditKind, text: &str) -> Result<Vec<Action>> {
        match kind {
            EditKind::Targets(discovery) => {
                // The import port is selected in the credentials step.
                let preview = targets::preview(text, [], self.config.settings.scan_port, discovery);
                self.modal = Some(Modal::Preview {
                    hosts: preview.hosts,
                    discovery,
                    duplicates: preview.duplicates,
                    errors: preview.errors,
                });
            }
            EditKind::Search => {
                self.search = text.trim().into();
                self.cursor = 0;
                self.scan_cursor = 0;
            }
            EditKind::Settings => {
                let settings: Settings = toml::from_str(text)?;
                let mut cfg = self.config.clone();
                cfg.settings = settings;
                self.persist(cfg)?;
                self.log("Settings saved. Connection concurrency and mouse capture changes apply after restart.");
            }
            EditKind::Server(id) => {
                let server = bmcforge_core::config::parse_server(text)?;
                ensure!(server.id == id, "server id is immutable");
                ensure!(
                    !self.snapshots.get(&id).is_some_and(|s| s.taken_over),
                    "restore automatic mode first"
                );
                if let Some(profile) = &server.profile {
                    ensure!(
                        self.registry.profiles.contains_key(profile),
                        "unknown profile ID"
                    );
                }
                let mut cfg = self.config.clone();
                let old = cfg
                    .servers
                    .iter_mut()
                    .find(|s| s.id == id)
                    .context("server missing")?;
                *old = server.clone();
                cfg.validate()?;
                return Ok(vec![Action::Configure(server)]);
            }
            EditKind::Manual(ids) => {
                let form: ManualForm = toml::from_str(text)?;
                self.confirm_control(ids, Some((form.percent, form.zone)))?;
            }
            EditKind::Credentials(hosts, authenticate) => {
                let form: ImportForm = toml::from_str(text)?;
                if let Some(id) = &form.profile {
                    ensure!(
                        self.registry.profiles.contains_key(id),
                        "unknown profile ID"
                    );
                }
                let mut servers = vec![];
                for host in hosts {
                    self.sequence += 1;
                    let server = Server {
                        id: format!(
                            "{}-{}-{}",
                            std::process::id(),
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_micros(),
                            self.sequence
                        ),
                        name: host.clone(),
                        host,
                        port: form.port,
                        credentials: Credentials {
                            username: form.username.clone(),
                            password: form.password.clone(),
                        },
                        tags: form.tags.clone(),
                        profile: form.profile.clone(),
                        allow_experimental: form.allow_experimental,
                        ..Default::default()
                    };
                    server.validate()?;
                    servers.push(server);
                }
                let before = servers.len();
                servers.retain(|s| {
                    !self.config.servers.iter().any(|old| {
                        targets::canonical_endpoint(&s.host, s.port)
                            == targets::canonical_endpoint(&old.host, old.port)
                    })
                });
                self.modal = Some(Modal::Import {
                    duplicates: before - servers.len(),
                    servers,
                    authenticate,
                });
            }
        }
        Ok(vec![])
    }
    fn export(&mut self) -> Result<()> {
        let diagnostics = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "servers": self.config.servers.iter().map(|server| {
            let snapshot = self.snapshots.get(&server.id);
            serde_json::json!({ "id": server.id, "name": server.name, "host": server.host, "profile": server.profile,
                "identity": snapshot.map(|s| &s.identity), "sensors": snapshot.map(|s| &s.sensors), "status": snapshot.map(|s| &s.status) })
        }).collect::<Vec<_>>(), "logs": self.logs });
        let parent = self.path.parent().unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(parent)?;
        let path = parent.join(format!(
            "diagnostics-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
        ));
        // Redact string values before serialization so escaped passwords cannot leak
        // and replacement cannot corrupt JSON syntax.
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&redact_value(diagnostics, &self.config.secrets()))?,
        )?;
        self.log(format!("Diagnostics saved: {}", path.display()));
        Ok(())
    }
}

fn redact_value(mut value: serde_json::Value, secrets: &[String]) -> serde_json::Value {
    match &mut value {
        serde_json::Value::String(s) => *s = redact(s, secrets),
        serde_json::Value::Array(items) => {
            for item in items {
                *item = redact_value(item.take(), secrets);
            }
        }
        serde_json::Value::Object(items) => {
            for item in items.values_mut() {
                *item = redact_value(item.take(), secrets);
            }
        }
        _ => {}
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let cfg = Config {
            servers: bmcforge_ipmi::mock::servers(),
            ..Default::default()
        };
        App::new(
            cfg,
            PathBuf::from("unused"),
            Registry::load(None).unwrap(),
            true,
        )
    }
    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    #[test]
    fn import_uses_the_selected_port_before_existing_endpoint_dedup() -> Result<()> {
        let mut a = app();
        let form = ImportForm {
            username: "root".into(),
            password: "test".into(),
            port: 624,
            tags: vec![],
            profile: None,
            allow_experimental: false,
        };
        a.submit(
            EditKind::Credentials(vec!["192.0.2.10".into()], false),
            &toml::to_string(&form)?,
        )?;
        assert_eq!(a.config.servers.len(), 2);
        assert!(
            matches!(&a.modal,Some(Modal::Import {servers,duplicates:0,..}) if servers.len()==1)
        );
        assert!(
            matches!(a.input(KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE)).as_slice(),[Action::Add(s)] if s.len()==1)
        );
        assert_eq!(a.config.servers.len(), 3);
        a.submit(
            EditKind::Credentials(vec!["192.0.2.10".into()], false),
            &toml::to_string(&form)?,
        )?;
        assert!(
            matches!(&a.modal,Some(Modal::Import {servers,duplicates:1,..}) if servers.is_empty())
        );
        Ok(())
    }
    #[test]
    fn stale_scan_messages_do_not_affect_the_new_scan() {
        let mut a = app();
        a.scan_generation = 2;
        a.scanning = true;
        a.message(Message::Discovered(
            1,
            DiscoveryResult {
                host: "192.0.2.1".into(),
                status: ProbeStatus::IpmiCandidate,
            },
        ));
        a.message(Message::ScanFinished(1));
        assert!(a.discovery.is_empty() && a.scanning);
        a.message(Message::Discovered(
            2,
            DiscoveryResult {
                host: "192.0.2.2".into(),
                status: ProbeStatus::IpmiCandidate,
            },
        ));
        a.message(Message::ScanFinished(2));
        assert_eq!(a.discovery.len(), 1);
        assert!(!a.scanning);
    }
    #[test]
    fn json_redaction_handles_escaped_quotes_and_newlines() -> Result<()> {
        let secret = "a\"b\\c\n密码".to_string();
        let value =
            serde_json::json!({"message":format!("error: {secret}"),"nested":[secret.clone()]});
        let sanitized = redact_value(value, std::slice::from_ref(&secret));
        assert_eq!(sanitized["message"], "error: [REDACTED]");
        let serialized = serde_json::to_string(&sanitized)?;
        assert!(!serialized.contains("密码"));
        let _: serde_json::Value = serde_json::from_str(&serialized)?;
        Ok(())
    }
    #[test]
    fn server_edits_wait_for_worker_ack_before_persisting() -> Result<()> {
        let mut a = app();
        let mut server = a.config.servers[0].clone();
        let id = server.id.clone();
        server.name = "new name".into();
        let actions = a.submit(EditKind::Server(id), &toml::to_string(&server)?)?;
        assert!(matches!(actions.as_slice(), [Action::Configure(_)]));
        assert_ne!(a.config.servers[0].name, "new name");
        a.message(Message::Configured(server, None));
        assert_eq!(a.config.servers[0].name, "new name");
        Ok(())
    }
    #[test]
    fn windows_release_does_not_trigger_and_selection_filters() {
        let mut a = app();
        let mut release = key('q');
        release.kind = KeyEventKind::Release;
        assert!(a.input(release).is_empty());
        a.input(key(' '));
        assert_eq!(a.selected.len(), 1);
        a.search = "Supermicro".into();
        assert_eq!(a.visible().len(), 1);
        assert!(matches!(a.input(key('q')).as_slice(), [Action::Quit]));
    }
    #[test]
    fn control_requires_preview_and_explicit_experimental_enablement() {
        let mut a = app();
        a.input(key('m'));
        assert!(matches!(a.modal, Some(Modal::Editor { .. })));
        let actions = a.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(actions.is_empty());
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        assert_eq!(
            a.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .len(),
            1
        );
        a.config.servers[0].allow_experimental = false;
        a.cursor = 0;
        a.input(key('m'));
        assert!(a.modal.is_none());
    }
}

use crate::{
    editor::Editor,
    runtime::{Command, Message, Snapshot},
};
use anyhow::{Context, Result, ensure};
use bmcforge_core::{
    ControlValue, DeviceIdentity,
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
    Configure(Box<Server>),
    Send(String, Command),
    Scan(Vec<String>, u16),
    CancelScan,
    Authenticate(Vec<Server>),
    CancelIdentify,
    Forget(String),
    Quit,
}
#[derive(Clone)]
pub enum EditKind {
    Targets(bool),
    Credentials(Vec<String>, bool),
    Server(String),
    Settings,
    Profile(String),
    Search,
    Manual(Vec<String>),
}
pub enum Modal {
    Actions {
        previous: Option<Box<Modal>>,
        items: Vec<(String, crate::mouse::Target)>,
        cursor: usize,
    },
    Commands {
        query: String,
        cursor: usize,
    },
    Form(crate::form::Form),
    FieldChoice {
        form: Box<crate::form::Form>,
        query: String,
        cursor: usize,
    },
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
    DiscoveryResults,
    Confirm {
        title: String,
        text: String,
        control: Vec<(String, Profile)>,
        manual: Option<(ControlValue, u8)>,
    },
    Languages {
        cursor: usize,
    },
    ModelChoice {
        query: String,
        cursor: usize,
    },
    Info(String),
    ProfileDetail {
        id: String,
        reveal: bool,
    },
    ResetProfile(String),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<u8>,
    zone: u8,
}
pub struct Identified {
    pub server: Server,
    pub identity: DeviceIdentity,
    pub candidates: Vec<bmcforge_profiles::models::Candidate>,
    pub error: Option<String>,
}

pub struct App {
    pub locales: crate::i18n::Locales,
    pub catalog: bmcforge_profiles::models::Catalog,
    pub hits: Vec<crate::mouse::Hit>,
    pub interaction: crate::interaction::Interaction,
    pub advanced_form: Option<crate::form::Form>,
    pub last_click: Option<(String, std::time::Instant)>,
    pub identify_generation: u64,
    pub identifying: bool,
    pub identify_total: usize,
    pub pending: Vec<Identified>,
    pub resolved: Vec<Server>,
    pub config: Config,
    pub path: PathBuf,
    pub registry: Registry,
    pub snapshots: BTreeMap<String, Snapshot>,
    pub history: BTreeMap<String, VecDeque<u64>>,
    pub selected: BTreeSet<String>,
    pub cursor: usize,
    pub page: usize,
    pub profile_cursor: usize,
    pub model_scroll: usize,
    pub scan_cursor: usize,
    pub discovery: Vec<DiscoveryResult>,
    pub discovery_selected: BTreeSet<String>,
    pub scan_total: usize,
    pub scan_generation: u64,
    pub scanning: bool,
    pub discovery_ready: bool,
    pub candidates_only: bool,
    pub search: String,
    pub logs: VecDeque<crate::i18n::Message>,
    pub modal: Option<Modal>,
    pub modal_scroll: u16,
    pub status: crate::i18n::Message,
    pub demo: bool,
    pub detail_scroll: u16,
    pub sensor_scroll: usize,
    pub log_scroll: u16,
    sequence: u64,
}
impl App {
    pub fn new(config: Config, path: PathBuf, registry: Registry, demo: bool) -> Self {
        Self {
            locales: crate::i18n::Locales::load(None).expect("builtin languages"),
            catalog: bmcforge_profiles::models::Catalog::load(None).expect("builtin models"),
            hits: vec![],
            interaction: Default::default(),
            advanced_form: None,
            last_click: None,
            identify_generation: 0,
            identifying: false,
            identify_total: 0,
            pending: vec![],
            resolved: vec![],
            config,
            path,
            registry,
            snapshots: BTreeMap::new(),
            history: BTreeMap::new(),
            selected: BTreeSet::new(),
            cursor: 0,
            page: 0,
            profile_cursor: 0,
            model_scroll: 0,
            scan_cursor: 0,
            discovery: vec![],
            discovery_selected: BTreeSet::new(),
            scan_total: 0,
            scan_generation: 0,
            scanning: false,
            discovery_ready: false,
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
    pub(crate) fn action_text(&self, action: &str) -> String {
        match action {
            "edit" => self
                .text(if self.page == 2 {
                    "edit-adapter"
                } else {
                    "edit-server"
                })
                .into(),
            "manual" | "automatic" | "select" => {
                format!("{} ({})", self.text(action), self.text("selected-servers"))
            }
            "import" if matches!(self.modal, Some(Modal::DiscoveryResults)) => {
                self.text("identify").into()
            }
            "discover" if self.scanning => self.text("cancel-discovery").into(),
            "search" => self.text("search-servers").into(),
            "export" => self.text("export-diagnostics").into(),
            _ => self.text(action).into(),
        }
    }
    pub(crate) fn modal_primary_label(&self) -> &str {
        match &self.modal {
            Some(Modal::Form(f)) => match f.kind {
                EditKind::Settings | EditKind::Server(_) | EditKind::Profile(_) => "save",
                EditKind::Search => "search",
                _ => "continue",
            },
            Some(Modal::Editor { kind, .. }) => match kind {
                EditKind::Settings | EditKind::Server(_) | EditKind::Profile(_) => "save",
                EditKind::Search => "search",
                _ => "continue",
            },
            Some(Modal::Info(_) | Modal::ProfileDetail { .. }) => "close",
            Some(Modal::ResetProfile(_)) => "reset-adapter",
            Some(Modal::Remove { .. }) => "remove-server-action",
            Some(Modal::DiscoveryResults) => "identify",
            Some(Modal::Import {
                authenticate: true, ..
            }) => "identify",
            Some(Modal::Import { .. }) => "import",
            Some(Modal::Preview {
                discovery: true, ..
            }) => "discover",
            Some(Modal::Preview { .. }) => "continue",
            Some(
                Modal::Languages { .. }
                | Modal::ModelChoice { .. }
                | Modal::FieldChoice { .. }
                | Modal::Commands { .. }
                | Modal::Actions { .. },
            ) => "choose",
            _ => "confirm",
        }
    }
    pub fn text<'a>(&'a self, id: &'a str) -> &'a str {
        self.locales.text(&self.config.settings.language, id)
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
    pub fn host_exists(&self, host: &str) -> bool {
        let host = targets::canonical_host(host);
        self.config
            .servers
            .iter()
            .any(|server| targets::canonical_host(&server.host) == host)
    }
    fn discovery_selectable(&self, host: &str) -> bool {
        !self.host_exists(host)
            && self
                .discovery
                .iter()
                .any(|result| result.host == host && result.status == ProbeStatus::IpmiCandidate)
    }
    pub(crate) fn toggle_discovery_selection(&mut self) {
        if let Some(index) = self.scan_visible().get(self.scan_cursor) {
            let host = self.discovery[*index].host.clone();
            if !self.discovery_selectable(&host) {
                self.discovery_selected.remove(&host);
                return;
            }
            if !self.discovery_selected.remove(&host) {
                self.discovery_selected.insert(host);
            }
        }
    }
    fn select_all_discovery(&mut self) {
        let hosts: Vec<_> = self
            .scan_visible()
            .iter()
            .filter_map(|i| {
                let r = &self.discovery[*i];
                (r.status == ProbeStatus::IpmiCandidate && !self.host_exists(&r.host))
                    .then(|| r.host.clone())
            })
            .collect();
        if hosts.iter().all(|h| self.discovery_selected.contains(h)) {
            for h in hosts {
                self.discovery_selected.remove(&h);
            }
        } else {
            self.discovery_selected.extend(hosts);
        }
    }
    fn select_discovery_group(&mut self) {
        if let Some(index) = self.scan_visible().get(self.scan_cursor) {
            let group = self.discovery[*index]
                .host
                .rsplit_once('.')
                .map(|(prefix, _)| prefix.to_owned());
            let hosts: Vec<_> = self
                .discovery
                .iter()
                .filter(|r| {
                    r.status == ProbeStatus::IpmiCandidate
                        && !self.host_exists(&r.host)
                        && r.host.rsplit_once('.').map(|(p, _)| p) == group.as_deref()
                })
                .map(|r| r.host.clone())
                .collect();
            self.discovery_selected.extend(hosts);
        }
    }
    pub fn log(&mut self, message: impl Into<crate::i18n::Message>) {
        let mut message = message.into();
        message.details = redact(&message.details, &self.config.secrets());
        for v in message.args.values_mut() {
            *v = redact(v, &self.config.secrets());
        }
        self.status.clone_from(&message);
        self.logs.push_back(message);
        while self.logs.len() > self.config.settings.history_capacity {
            self.logs.pop_front();
        }
    }
    pub(crate) fn persist(&mut self, updated: Config) -> Result<()> {
        updated.validate()?;
        if !self.demo {
            crate::local_profiles::ensure_clean(&self.path)?;
            updated.save(&self.path)?;
        }
        self.config = updated;
        Ok(())
    }
    pub fn add_servers(&mut self, servers: Vec<Server>) -> Result<Vec<Server>> {
        let mut cfg = self.config.clone();
        let mut added = vec![];
        for server in servers {
            if cfg
                .servers
                .iter()
                .any(|s| targets::canonical_host(&s.host) == targets::canonical_host(&server.host))
            {
                continue;
            }
            cfg.servers.push(server.clone());
            added.push(server);
        }
        self.persist(cfg)?;
        let mut event = crate::i18n::Message::new("added-count");
        event.args.insert("count".into(), added.len().to_string());
        self.log(event);
        Ok(added)
    }
    pub fn message(&mut self, message: Message) -> Vec<Action> {
        match message {
            Message::Snapshot(id, mut snapshot) => {
                if !self.config.servers.iter().any(|s| s.id == id) {
                    return vec![];
                }
                snapshot.status.details = redact(&snapshot.status.details, &self.config.secrets());
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
                if generation == self.scan_generation && self.scanning {
                    self.discovery.push(result);
                }
            }
            Message::ScanFinished(generation)
                if generation == self.scan_generation && self.scanning =>
            {
                self.scanning = false;
                self.discovery_ready = true;
                if self.modal.is_none() {
                    self.modal = Some(Modal::DiscoveryResults);
                    self.discovery_ready = false;
                }
                self.modal_scroll = 0;
                self.log(crate::i18n::Message::new("scan-finished"));
            }
            Message::ScanFinished(_) => {}
            Message::Configured(server, error) => {
                if let Some(error) = error {
                    self.log(format!("configuration unchanged: {error}"));
                } else {
                    let mut cfg = self.config.clone();
                    if let Some(old) = cfg.servers.iter_mut().find(|s| s.id == server.id) {
                        if *old == server {
                            self.log(crate::i18n::Message::new("config-restored"));
                            return vec![];
                        }
                        let rollback = old.clone();
                        *old = server;
                        if let Err(error) = self.persist(cfg) {
                            self.log(format!(
                                "save failed, rolling back running configuration: {error:#}"
                            ));
                            return vec![Action::Configure(Box::new(rollback))];
                        }
                        self.log(crate::i18n::Message::new("server-saved"));
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
                    self.log(crate::i18n::Message::new("server-removed"));
                    return vec![Action::Forget(id)];
                }
            }
            Message::Identified(generation, server, identity, error)
                if generation == self.identify_generation && self.identifying =>
            {
                let candidates = self.catalog.candidates(&identity);
                let error = error.map(|e| {
                    redact(
                        &e,
                        &[
                            server.credentials.password.clone(),
                            server.kg.clone().unwrap_or_default(),
                        ],
                    )
                });
                self.pending.push(Identified {
                    server,
                    identity,
                    candidates,
                    error,
                });
            }
            Message::IdentificationFinished(generation)
                if generation == self.identify_generation && self.identifying =>
            {
                self.identifying = false;
                self.log(crate::i18n::Message::new("identification-finished"));
                if self.modal.is_none() {
                    self.resolve_next();
                }
            }
            Message::Identified(..) | Message::IdentificationFinished(_) => {}
        }
        vec![]
    }
    fn edit(&mut self, title: impl Into<String>, kind: EditKind, text: &str) {
        match crate::form::Form::new(title.into(), kind, text) {
            Ok(mut form) => {
                self.enrich_form(&mut form);
                self.modal = Some(Modal::Form(form));
            }
            Err(e) => self.log(format!("{e:#}")),
        }
    }
    fn enrich_form(&self, form: &mut crate::form::Form) {
        use crate::form::FieldType;
        for field in &mut form.fields {
            let key = field.path.last().unwrap().as_str();
            let options = match key {
                "language" => Some(self.locales.packs.keys().cloned().collect::<Vec<_>>()),
                "profile" => Some(
                    std::iter::once(String::new())
                        .chain(self.registry.profiles.keys().cloned())
                        .collect(),
                ),
                _ => None,
            };
            if let Some(mut options) = options {
                if !options.contains(&field.value()) {
                    options.push(field.value());
                }
                field.kind = FieldType::Choice(options);
            }
            if let EditKind::Manual(ids) = &form.kind
                && let Some(fan) = ids
                    .first()
                    .and_then(|id| self.profile(id).ok())
                    .and_then(|p| p.fan)
            {
                let options = match key {
                    "raw" => fan
                        .raw_values
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    "percent" => fan.percent_values.iter().map(ToString::to_string).collect(),
                    "zone" => fan.zones.iter().map(|z| z.id.to_string()).collect(),
                    _ => vec![],
                };
                if !options.is_empty() {
                    field.kind = FieldType::Choice(options);
                } else if key == "percent" {
                    field.kind = FieldType::Number {
                        min: fan.min_percent.into(),
                        max: fan.max_percent.into(),
                    };
                }
            }
        }
    }
    fn validate_draft(&self, kind: &EditKind, text: &str) -> Result<()> {
        match kind {
            EditKind::Profile(id) => {
                let p = Profile::parse(text)?;
                ensure!(&p.id == id, "adapter ID is immutable");
                ensure!(
                    p.override_builtin == self.registry.profiles[id].override_builtin,
                    "adapter source is immutable"
                );
            }
            EditKind::Settings => {
                let mut cfg = self.config.clone();
                cfg.settings = toml::from_str(text)?;
                for (name, key) in &cfg.settings.keys {
                    if self.config.settings.keys.get(name) != Some(key) {
                        ensure!(
                            !"12345jkAGfcu".contains(key.as_str()),
                            "{name}: {}",
                            self.text("form-error-shortcut")
                        );
                    }
                }
                cfg.validate()?;
            }
            EditKind::Server(id) => {
                let server = bmcforge_core::config::parse_server(text)?;
                ensure!(&server.id == id, "server id is immutable");
                if let Some(profile) = &server.profile {
                    ensure!(
                        self.registry.profiles.contains_key(profile),
                        "unknown profile ID"
                    );
                }
                let mut cfg = self.config.clone();
                *cfg.servers
                    .iter_mut()
                    .find(|s| &s.id == id)
                    .context("server missing")? = server;
                cfg.validate()?;
            }
            EditKind::Credentials(..) => {
                let f: ImportForm = toml::from_str(text)?;
                if let Some(profile) = &f.profile {
                    ensure!(
                        self.registry.profiles.contains_key(profile),
                        "unknown profile ID"
                    );
                }
                Server {
                    id: "draft".into(),
                    host: "draft".into(),
                    port: f.port,
                    credentials: Credentials {
                        username: f.username,
                        password: f.password,
                    },
                    ..Default::default()
                }
                .validate()?;
            }
            EditKind::Manual(ids) => {
                let f: ManualForm = toml::from_str(text)?;
                let value = match (f.percent, f.raw) {
                    (Some(v), None) => ControlValue::Percent(v),
                    (None, Some(v)) => ControlValue::Raw(v),
                    _ => anyhow::bail!("specify exactly one of percent or raw"),
                };
                for id in ids {
                    self.profile(id)?.commands_value(value, f.zone, true)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shortcut(&self, key: KeyEvent, action: &str) -> bool {
        key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            && matches!(key.code, KeyCode::Char(c) if self.config.settings.keys.get(action).is_some_and(|s| s == &c.to_string()))
    }
    pub fn input(&mut self, key: KeyEvent) -> Vec<Action> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return vec![];
        }
        if let Some(actions) = self.menu_input(key) {
            return actions;
        }
        if let Some(actions) = self.command_input(key) {
            return actions;
        }
        if self
            .interaction
            .viewport
            .as_ref()
            .is_some_and(|(_, page, modal)| {
                *page != self.page || *modal != self.modal.as_ref().map(std::mem::discriminant)
            })
        {
            self.interaction.action_focus = None;
            self.interaction.actions.clear();
        }
        if key.code == KeyCode::F(8) && !self.interaction.actions.is_empty() {
            self.interaction.action_focus = if self.interaction.action_focus.is_some() {
                None
            } else {
                Some(0)
            };
            return vec![];
        }
        if let Some(index) = self.interaction.action_focus {
            let count = self.interaction.actions.len().max(1);
            match key.code {
                KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
                    self.interaction.action_focus = Some((index + count - 1) % count);
                    return vec![];
                }
                KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                    self.interaction.action_focus = Some((index + 1) % count);
                    return vec![];
                }
                KeyCode::Esc => {
                    self.interaction.action_focus = None;
                    return vec![];
                }
                KeyCode::Enter => {
                    let target = self.interaction.actions.get(index).cloned();
                    self.interaction.action_focus = None;
                    if let Some(target) = target {
                        return self.invoke_target(target);
                    }
                    return vec![];
                }
                _ => {
                    self.interaction.action_focus = None;
                }
            }
        }
        if self.modal.is_some() {
            let actions = self.modal_input(key);
            if self.modal.is_none() && self.discovery_ready {
                self.modal = Some(Modal::DiscoveryResults);
                self.discovery_ready = false;
                self.modal_scroll = 0;
            }
            self.keyboard_feedback(key);
            return actions;
        }
        let result = self.normal_input(key);
        self.keyboard_feedback(key);
        match result {
            Ok(actions) => actions,
            Err(error) => {
                self.log(format!("{error:#}"));
                vec![]
            }
        }
    }
    pub(crate) fn invoke_target(&mut self, target: crate::mouse::Target) -> Vec<Action> {
        if self.modal.is_some()
            && !matches!(
                target,
                crate::mouse::Target::Key(_)
                    | crate::mouse::Target::Overflow
                    | crate::mouse::Target::Discovery(..)
            )
            && !matches!(
                &target,
                crate::mouse::Target::Shortcut(name)
                    if name == "import" && matches!(self.modal, Some(Modal::DiscoveryResults))
            )
        {
            return vec![];
        }
        let reason = self.target_reason(&target);
        if !reason.is_empty() {
            self.log(reason);
            return vec![];
        }
        self.interaction.press(target.clone());
        let result = match target {
            crate::mouse::Target::Palette => {
                self.open_commands(String::new());
                Ok(vec![])
            }
            crate::mouse::Target::Status => {
                self.modal_scroll = 0;
                self.modal = Some(Modal::Info(format!(
                    "{}\n\n{}",
                    self.status_text(),
                    self.text("ui-copy-hint")
                )));
                Ok(vec![])
            }
            crate::mouse::Target::Overflow => {
                let items = self.interaction.menu_items.clone();
                let previous = self.modal.take().map(Box::new);
                self.modal = Some(Modal::Actions {
                    previous,
                    items,
                    cursor: 0,
                });
                self.interaction.action_focus = None;
                Ok(vec![])
            }

            crate::mouse::Target::Shortcut(name) => self.normal_action(&name),
            crate::mouse::Target::Page(page) => {
                self.page = page;
                Ok(vec![])
            }
            crate::mouse::Target::Language => {
                self.modal = Some(Modal::Languages { cursor: 0 });
                Ok(vec![])
            }
            crate::mouse::Target::Key(code) if self.modal.is_some() => {
                return self.input(KeyEvent::new(code, KeyModifiers::NONE));
            }
            crate::mouse::Target::Key(code) => {
                self.fixed_input(KeyEvent::new(code, KeyModifiers::NONE))
            }
            _ => Ok(vec![]),
        };
        match result {
            Ok(actions) => actions,
            Err(error) => {
                self.log(format!("{error:#}"));
                vec![]
            }
        }
    }
    fn normal_input(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        for action in crate::interaction::ACTIONS {
            if self.shortcut(key, action) {
                return self.normal_action(action);
            }
        }
        self.fixed_input(key)
    }
    pub(crate) fn command_reason(&self, action: &str) -> String {
        let result: Result<()> = (|| {
            if action == "import" {
                ensure!(!self.identifying, "{}", self.text("identifying"));
            }
            if action == "edit"
                && self.page != 2
                && let Some(server) = self.current()
            {
                ensure!(
                    !self.snapshots.get(&server.id).is_some_and(|s| s.taken_over),
                    "{}",
                    self.text("automatic")
                );
            }
            if matches!(action, "manual" | "automatic") {
                let ids = self.targets();
                for id in ids {
                    let profile = self.profile(&id)?;
                    ensure!(profile.fan.is_some(), "{}", self.text("monitor-only"));
                }
            }
            Ok(())
        })();
        result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }
    fn normal_action(&mut self, action: &str) -> Result<Vec<Action>> {
        if action == "quit" {
            return Ok(vec![Action::Quit]);
        }
        if action == "search" {
            self.edit(
                self.text("search-servers-tags-addresses").to_owned(),
                EditKind::Search,
                &self.search.clone(),
            );
            return Ok(vec![]);
        }
        if action == "import" {
            ensure!(
                !self.identifying,
                "cancel or finish the current identification batch first"
            );
            if matches!(self.modal, Some(Modal::DiscoveryResults)) {
                let hosts: Vec<_> = self
                    .discovery
                    .iter()
                    .filter(|r| {
                        r.status == ProbeStatus::IpmiCandidate
                            && self.discovery_selected.contains(&r.host)
                            && !self.host_exists(&r.host)
                    })
                    .map(|r| r.host.clone())
                    .collect();
                ensure!(!hosts.is_empty(), "no IPMI candidates selected");
                self.credentials(hosts, true)?;
            } else {
                self.edit(
                    self.text("import-ip-hostname-cidr-range-one-per-line")
                        .to_owned(),
                    EditKind::Targets(false),
                    "",
                );
            }
            return Ok(vec![]);
        }
        if action == "discover" {
            if self.scanning {
                self.scanning = false;
                return Ok(vec![Action::CancelScan]);
            }
            self.edit(
                self.text("discover-ipv4-cidr-range-one-per-line")
                    .to_owned(),
                EditKind::Targets(true),
                if self.demo {
                    "192.0.2.20-192.0.2.23"
                } else {
                    ""
                },
            );
            return Ok(vec![]);
        }
        if action == "settings" {
            self.edit(
                self.text("settings").to_owned(),
                EditKind::Settings,
                &toml::to_string_pretty(&self.config.settings)?,
            );
            return Ok(vec![]);
        }
        if action == "profiles" {
            self.page = 2;
            return Ok(vec![]);
        }
        if action == "logs" {
            self.page = 3;
            return Ok(vec![]);
        }
        if action == "export" {
            self.export()?;
            return Ok(vec![]);
        }
        if action == "edit" && self.page == 2 {
            let id = self
                .registry
                .profiles
                .keys()
                .nth(self.profile_cursor)
                .context("select adapter")?
                .clone();
            self.edit_adapter(&id)?;
            return Ok(vec![]);
        }
        if action == "reset-adapter" {
            let id = self
                .registry
                .profiles
                .keys()
                .nth(self.profile_cursor)
                .context("select adapter")?
                .clone();
            ensure!(
                self.registry.local.contains_key(&id),
                "select a local adapter"
            );
            self.modal_scroll = 0;
            self.modal = Some(Modal::ResetProfile(id));
            return Ok(vec![]);
        }
        if action == "edit" {
            let server = self.current().context("select a server first")?;
            ensure!(
                !self.snapshots.get(&server.id).is_some_and(|s| s.taken_over),
                "restore automatic mode before editing this server"
            );
            self.edit(
                self.text("server-settings-contains-plaintext-password")
                    .to_owned(),
                EditKind::Server(server.id.clone()),
                &toml::to_string_pretty(server)?,
            );
            return Ok(vec![]);
        }
        if action == "manual" {
            let ids = self.targets();
            ensure!(!ids.is_empty(), "select a server first");
            let profile = self.profile(&ids[0])?;
            let zone = profile.fan.as_ref().context("monitoring only")?.zones[0].id;
            let raw = profile
                .fan
                .as_ref()
                .and_then(|f| f.raw_values.first().copied());
            let percent = profile
                .fan
                .as_ref()
                .and_then(|f| f.percent_values.first().copied())
                .unwrap_or(50);
            self.edit(
                self.text("fixed-duty-percentage-and-zone-id").to_owned(),
                EditKind::Manual(ids),
                &toml::to_string_pretty(&ManualForm {
                    percent: raw.is_none().then_some(percent),
                    raw,
                    zone,
                })?,
            );
            return Ok(vec![]);
        }
        if action == "automatic" {
            self.confirm_control(self.targets(), None)?;
            return Ok(vec![]);
        }
        if action == "select" {
            if matches!(self.modal, Some(Modal::DiscoveryResults)) {
                self.toggle_discovery_selection();
            } else if let Some(server) = self.current() {
                let id = server.id.clone();
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
            }
            return Ok(vec![]);
        }
        Ok(vec![])
    }
    /// Shared fixed actions; mouse buttons do not reinterpret user-remapped character shortcuts.
    pub(crate) fn fixed_input(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        match key.code {
            KeyCode::F(3) => self.modal = Some(Modal::Languages { cursor: 0 }),
            KeyCode::Char('A') if matches!(self.modal, Some(Modal::DiscoveryResults)) => {
                self.select_all_discovery();
            }
            KeyCode::Char('G') if matches!(self.modal, Some(Modal::DiscoveryResults)) => {
                self.select_discovery_group();
            }
            KeyCode::Char('c') if self.identifying => {
                self.identifying = false;
                self.pending.clear();
                return Ok(vec![Action::CancelIdentify]);
            }
            KeyCode::Delete => {
                if let Some(server) = self.current() {
                    self.modal = Some(Modal::Remove {
                        id: server.id.clone(),
                        name: server.name.clone(),
                    });
                }
            }
            KeyCode::F(1) => {
                self.page = 4;
            }
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
                if self.page == 3 {
                    self.log_scroll = self.log_scroll.saturating_add(10);
                } else if self.page == 2 {
                    self.model_scroll = self.model_scroll.saturating_add(8);
                } else {
                    if self.page == 1 {
                        self.sensor_scroll = self.sensor_scroll.saturating_add(10);
                    } else {
                        self.detail_scroll = self.detail_scroll.saturating_add(10);
                    }
                }
            }
            KeyCode::PageUp => {
                if self.page == 3 {
                    self.log_scroll = self.log_scroll.saturating_sub(10);
                } else if self.page == 2 {
                    self.model_scroll = self.model_scroll.saturating_sub(8);
                } else {
                    if self.page == 1 {
                        self.sensor_scroll = self.sensor_scroll.saturating_sub(10);
                    } else {
                        self.detail_scroll = self.detail_scroll.saturating_sub(10);
                    }
                }
            }
            KeyCode::Enter if self.page == 0 => {
                if self
                    .interaction
                    .viewport
                    .as_ref()
                    .is_some_and(|(area, _, _)| area.width < 80)
                {
                    self.modal = Some(Modal::Info(crate::ui::server_detail(self)));
                } else {
                    self.page = 1;
                }
            }
            KeyCode::Enter if self.page == 2 => {
                if let Some(profile) = self.registry.profiles.values().nth(self.profile_cursor) {
                    self.modal_scroll = 0;
                    self.modal = Some(Modal::ProfileDetail {
                        id: profile.id.clone(),
                        reveal: false,
                    });
                }
            }
            KeyCode::Char('u') if self.page == 2 => self.default_credential()?,
            _ => {}
        }
        Ok(vec![])
    }
    fn move_cursor(&mut self, down: bool) {
        let (cursor, count) = if self.page == 2 {
            (&mut self.profile_cursor, self.registry.profiles.len())
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
        self.model_scroll = 0;
    }
    fn profile(&self, id: &str) -> Result<Profile> {
        let server = self
            .config
            .servers
            .iter()
            .find(|s| s.id == id)
            .context("server missing")?;
        let profile = if let Some(id) = &server.profile {
            self.registry
                .local
                .values()
                .find(|l| &l.origin_id == id)
                .map(|l| l.profile.id.clone())
                .unwrap_or_else(|| id.clone())
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
    fn edit_adapter(&mut self, id: &str) -> Result<()> {
        let effective = self
            .registry
            .local
            .values()
            .find(|l| l.origin_id == id)
            .map(|l| l.profile.id.as_str())
            .unwrap_or(id);
        let p = self
            .registry
            .profiles
            .get(effective)
            .context("adapter missing")?
            .clone();
        self.edit(
            self.text("edit-adapter").to_owned(),
            EditKind::Profile(p.id.clone()),
            &toml::to_string_pretty(&p)?,
        );
        Ok(())
    }
    pub(crate) fn adapter_affected(&self, id: &str) -> Vec<String> {
        let Some(previous) = self.registry.profiles.get(id) else {
            return vec![];
        };
        let origin = self
            .registry
            .local
            .get(id)
            .map(|l| l.origin_id.as_str())
            .unwrap_or(id);
        self.config
            .servers
            .iter()
            .filter(|s| {
                s.profile.as_deref().is_some_and(|p| p == id || p == origin)
                    || (s.profile.is_none()
                        && self.snapshots.get(&s.id).is_some_and(|snap| {
                            previous.matches(&snap.identity)
                                || self
                                    .registry
                                    .profiles
                                    .get(origin)
                                    .is_some_and(|p| p.matches(&snap.identity))
                        }))
            })
            .map(|s| s.id.clone())
            .collect()
    }
    fn save_adapter(&mut self, id: &str, edited: Option<Profile>) -> Result<Vec<Action>> {
        let previous = self
            .registry
            .profiles
            .get(id)
            .context("adapter missing")?
            .clone();
        if let Some(p) = &edited {
            p.validate()?;
            ensure!(
                p.id == previous.id && p.override_builtin == previous.override_builtin,
                "adapter identity is immutable"
            );
            if toml::to_string(p)? == toml::to_string(&previous)? {
                return Ok(vec![]);
            }
        }
        let mut local = self.registry.local.clone();
        let origin = local
            .get(id)
            .map(|l| l.origin_id.clone())
            .unwrap_or_else(|| id.into());
        let affected = self.adapter_affected(id);
        ensure!(
            !self.identifying,
            "finish identification before changing adapters"
        );
        ensure!(
            !affected
                .iter()
                .any(|id| self.snapshots.get(id).is_some_and(|s| s.taken_over)),
            "restore automatic mode before changing this adapter"
        );
        let target = if let Some(mut p) = edited {
            let existing = local
                .values()
                .find(|l| l.origin_id == origin)
                .map(|l| l.profile.id.clone());
            let target = existing.unwrap_or_else(|| {
                let mut n = 1;
                loop {
                    let next = format!("{}-local-{n}", origin);
                    if !self.registry.profiles.contains_key(&next) {
                        break next;
                    }
                    n += 1;
                }
            });
            p.id = target.clone();
            p.origin_id = Some(origin.clone());
            let original = local
                .get(&target)
                .map(|l| l.original.clone())
                .unwrap_or_else(|| previous.clone());
            local.insert(
                target.clone(),
                bmcforge_profiles::LocalProfile {
                    origin_id: origin.clone(),
                    original,
                    profile: p,
                },
            );
            target
        } else {
            ensure!(
                local.remove(id).is_some(),
                "only local adapters can be reset"
            );
            ensure!(
                self.registry.profiles.contains_key(&origin),
                "original adapter unavailable"
            );
            origin.clone()
        };
        let mut config = self.config.clone();
        for server in &mut config.servers {
            if server
                .profile
                .as_deref()
                .is_some_and(|p| p == id || p == origin)
            {
                server.profile = Some(target.clone());
            }
        }
        if !self.demo {
            crate::local_profiles::save(&self.path, &config, &local)?;
        }
        for key in self.registry.local.keys() {
            self.registry.profiles.remove(key);
        }
        for (key, entry) in &local {
            self.registry
                .profiles
                .insert(key.clone(), entry.profile.clone());
        }
        self.registry.local = local;
        self.config = config;
        self.profile_cursor = self
            .registry
            .profiles
            .keys()
            .position(|p| p == &target)
            .unwrap_or(0);
        Ok(self
            .config
            .servers
            .iter()
            .filter(|s| affected.contains(&s.id))
            .cloned()
            .map(|s| Action::Configure(Box::new(s)))
            .collect())
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
    fn confirm_control(
        &mut self,
        ids: Vec<String>,
        manual: Option<(ControlValue, u8)>,
    ) -> Result<()> {
        ensure!(!ids.is_empty(), "no server selected");
        let mut profiles = vec![];
        let mut lines = vec![];
        for id in ids {
            let profile = self.profile(&id)?;
            if let Some((value, zone)) = manual {
                profile.commands_value(value, zone, true)?;
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
            .map(|(p, z)| match p {
                ControlValue::Percent(v) => format!(
                    "{}: {v}% / {z} | {}: {}",
                    self.text("manual"),
                    self.text("actual-duty"),
                    self.text("unknown")
                ),
                ControlValue::Raw(v) => format!(
                    "{}: {v:#04x} / {z} | {}: {}",
                    self.text("raw"),
                    self.text("actual-duty"),
                    self.text("unknown")
                ),
            })
            .unwrap_or_else(|| self.text("automatic").into());
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
            self.text("import-credentials-plaintext-optional-profile-id")
                .to_owned(),
            EditKind::Credentials(hosts, authenticated),
            &toml::to_string_pretty(&form)?,
        );
        Ok(())
    }
    pub fn paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Editor { editor, .. }) => editor.paste(text),
            Some(Modal::Form(form)) => form.paste(text),
            Some(Modal::Commands { query, cursor } | Modal::FieldChoice { query, cursor, .. }) => {
                query.push_str(&text.replace(['\r', '\n'], ""));
                *cursor = 0;
            }
            _ => {}
        }
    }
    fn modal_input(&mut self, key: KeyEvent) -> Vec<Action> {
        if key.code == KeyCode::Esc {
            if matches!(self.modal, Some(Modal::FieldChoice { .. })) {
                if let Some(Modal::FieldChoice { form, .. }) = self.modal.take() {
                    self.modal = Some(Modal::Form(*form));
                }
                self.modal_scroll = 0;
                return vec![];
            }
            if matches!(self.modal, Some(Modal::ModelChoice { .. })) {
                self.pending.clear();
                self.resolved.clear();
            }
            self.modal = None;
            self.advanced_form = None;
            self.modal_scroll = 0;
            return vec![];
        }
        if !matches!(self.modal, Some(Modal::Editor { .. } | Modal::Form(_))) {
            if key.code == KeyCode::PageDown {
                if matches!(self.modal, Some(Modal::DiscoveryResults)) {
                    self.scan_cursor =
                        (self.scan_cursor + 10).min(self.scan_visible().len().saturating_sub(1));
                    return vec![];
                }
                self.modal_scroll = self.modal_scroll.saturating_add(8);
                return vec![];
            }
            if key.code == KeyCode::PageUp {
                if matches!(self.modal, Some(Modal::DiscoveryResults)) {
                    self.scan_cursor = self.scan_cursor.saturating_sub(10);
                    return vec![];
                }
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
            Modal::Commands { .. } | Modal::Actions { .. } => vec![],
            Modal::DiscoveryResults => match key.code {
                KeyCode::Esc => vec![],
                KeyCode::Up => {
                    self.scan_cursor = self.scan_cursor.saturating_sub(1);
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Down => {
                    self.scan_cursor =
                        (self.scan_cursor + 1).min(self.scan_visible().len().saturating_sub(1));
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Char(' ') => {
                    self.toggle_discovery_selection();
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Char('A') => {
                    self.select_all_discovery();
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Char('G') => {
                    self.select_discovery_group();
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Char('f') => {
                    self.candidates_only = !self.candidates_only;
                    self.scan_cursor = 0;
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
                KeyCode::Enter => self.normal_action("import").unwrap_or_else(|error| {
                    self.modal = Some(Modal::DiscoveryResults);
                    self.log(format!("{error:#}"));
                    vec![]
                }),
                KeyCode::Char(c)
                    if self.config.settings.keys.get("import") == Some(&c.to_string()) =>
                {
                    self.normal_action("import").unwrap_or_else(|error| {
                        self.modal = Some(Modal::DiscoveryResults);
                        self.log(format!("{error:#}"));
                        vec![]
                    })
                }
                _ => {
                    self.modal = Some(Modal::DiscoveryResults);
                    vec![]
                }
            },
            Modal::FieldChoice {
                mut form,
                mut query,
                mut cursor,
            } => {
                let options = form.choice_options(&query);
                match key.code {
                    KeyCode::Up => cursor = cursor.saturating_sub(1),
                    KeyCode::Down => cursor = (cursor + 1).min(options.len().saturating_sub(1)),
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        query.push(c);
                        cursor = 0;
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        cursor = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(option) = options.get(cursor) {
                            form.fields[form.focus].input = Editor::new(option);
                            form.fields[form.focus].error.clear();
                            form.error.clear();
                            self.modal = Some(Modal::Form(*form));
                            return vec![];
                        }
                    }
                    _ => {}
                }
                self.modal = Some(Modal::FieldChoice {
                    form,
                    query,
                    cursor,
                });
                vec![]
            }
            Modal::Form(mut form) => {
                if matches!(form.kind, EditKind::Settings)
                    && matches!(key.code, KeyCode::F(4) | KeyCode::F(5))
                {
                    if let Err(error) = form.reset_settings(key.code == KeyCode::F(5)) {
                        form.locate_error(&error.to_string());
                    }
                    self.enrich_form(&mut form);
                    self.modal = Some(Modal::Form(form));
                    return vec![];
                }

                if matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
                    && matches!(
                        form.fields[form.focus].kind,
                        crate::form::FieldType::Choice(_)
                    )
                    && key.modifiers.is_empty()
                {
                    let cursor = form
                        .choice_options("")
                        .iter()
                        .position(|v| v == &form.fields[form.focus].value())
                        .unwrap_or(0);
                    self.modal = Some(Modal::FieldChoice {
                        form: Box::new(form),
                        query: String::new(),
                        cursor,
                    });
                    return vec![];
                }
                let submit = key.code == KeyCode::F(2)
                    || (key.code == KeyCode::Enter
                        && (key.modifiers.contains(KeyModifiers::CONTROL)
                            || matches!(form.kind, EditKind::Search)));
                if key.code == KeyCode::F(7) && form.advanced {
                    match form.text() {
                        Ok(text) => {
                            self.modal = Some(Modal::Editor {
                                title: format!("{} — {}", form.title, self.text("form-advanced")),
                                kind: form.kind.clone(),
                                editor: Editor::new(&text),
                                error: String::new(),
                            });
                            self.advanced_form = Some(form);
                        }
                        Err(e) => {
                            form.error = e.to_string();
                            self.modal = Some(Modal::Form(form));
                        }
                    }
                    return vec![];
                }
                if submit {
                    let result = form.text().and_then(|text| {
                        self.validate_draft(&form.kind, &text)?;
                        self.submit(form.kind.clone(), &text)
                    });
                    match result {
                        Ok(actions) => return actions,
                        Err(e) => form.locate_error(&e.to_string()),
                    }
                } else {
                    form.input(key);
                }
                self.modal = Some(Modal::Form(form));
                vec![]
            }
            Modal::Languages { mut cursor } => {
                let ids: Vec<_> = self.locales.packs.keys().cloned().collect();
                match key.code {
                    KeyCode::Down => cursor = (cursor + 1).min(ids.len().saturating_sub(1)),
                    KeyCode::Up => cursor = cursor.saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(id) = ids.get(cursor) {
                            let mut cfg = self.config.clone();
                            cfg.settings.language = id.clone();
                            if let Err(e) = self.persist(cfg) {
                                self.log(format!("{e:#}"));
                            }
                        }
                        return vec![];
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Languages { cursor });
                vec![]
            }
            Modal::ModelChoice {
                mut query,
                mut cursor,
            } => {
                let options = self.model_options(&query);
                match key.code {
                    KeyCode::Down => cursor = (cursor + 1).min(options.len().saturating_sub(1)),
                    KeyCode::Up => cursor = cursor.saturating_sub(1),
                    KeyCode::Backspace => {
                        query.pop();
                        cursor = 0;
                    }
                    KeyCode::F(6) => {
                        self.pending.remove(0);
                        self.resolve_next();
                        return vec![];
                    }
                    KeyCode::F(5) => {
                        self.accept_model(None);
                        self.resolve_next();
                        return vec![];
                    }
                    KeyCode::Char(c) => {
                        query.push(c);
                        cursor = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(id) = options.get(cursor) {
                            self.accept_model(Some(id.clone()));
                            self.resolve_next();
                            return vec![];
                        }
                    }
                    _ => {}
                }
                self.modal = Some(Modal::ModelChoice { query, cursor });
                vec![]
            }
            Modal::Editor {
                title,
                kind,
                mut editor,
                ..
            } => {
                if key.code == KeyCode::F(7) && self.advanced_form.is_some() {
                    let result = self.validate_draft(&kind, &editor.text()).and_then(|()| {
                        crate::form::Form::new(
                            self.advanced_form.as_ref().unwrap().title.clone(),
                            kind.clone(),
                            &editor.text(),
                        )
                    });
                    match result {
                        Ok(mut form) => {
                            self.enrich_form(&mut form);
                            self.advanced_form = None;
                            self.modal = Some(Modal::Form(form));
                        }
                        Err(e) => {
                            self.modal = Some(Modal::Editor {
                                title,
                                kind,
                                editor,
                                error: e.to_string(),
                            })
                        }
                    }
                    return vec![];
                }
                if key.code == KeyCode::F(2)
                    || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    match self
                        .validate_draft(&kind, &editor.text())
                        .and_then(|()| self.submit(kind.clone(), &editor.text()))
                    {
                        Ok(actions) => {
                            self.advanced_form = None;
                            actions
                        }
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
                    self.modal = Some(Modal::Info(self.text("invalid-lines").into()));
                    return vec![];
                }
                if discovery {
                    self.discovery.clear();
                    self.discovery_selected.clear();
                    self.scan_cursor = 0;
                    self.scan_total = hosts.len();
                    self.scanning = true;
                    self.discovery_ready = false;
                    self.modal = None;
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
                                if let Some((value, zone)) = manual {
                                    Command::ManualValue(Box::new(p), value, zone)
                                } else {
                                    Command::Automatic(Box::new(p))
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
            Modal::ProfileDetail { id, mut reveal } => {
                match key.code {
                    KeyCode::Enter => {}
                    KeyCode::F(6) => {
                        reveal = !reveal;
                        self.modal = Some(Modal::ProfileDetail { id, reveal });
                    }
                    KeyCode::Char('e') => {
                        if let Err(error) = self.edit_adapter(&id) {
                            self.status = error.to_string().into();
                        }
                    }
                    KeyCode::Char('r') if self.registry.local.contains_key(&id) => {
                        self.modal_scroll = 0;
                        self.modal = Some(Modal::ResetProfile(id))
                    }
                    _ => self.modal = Some(Modal::ProfileDetail { id, reveal }),
                }
                vec![]
            }
            Modal::ResetProfile(id) => {
                if key.code == KeyCode::Enter {
                    match self.save_adapter(&id, None) {
                        Ok(actions) => actions,
                        Err(error) => {
                            self.status = error.to_string().into();
                            self.modal = Some(Modal::ResetProfile(id));
                            vec![]
                        }
                    }
                } else {
                    self.modal = Some(Modal::ResetProfile(id));
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
                        self.pending.clear();
                        self.resolved.clear();
                        self.identifying = true;
                        self.identify_total = servers.len();
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
                let existing = self
                    .config
                    .servers
                    .iter()
                    .map(|server| server.host.clone())
                    .collect::<Vec<_>>();
                let preview =
                    targets::preview(text, existing, self.config.settings.scan_port, discovery);
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
            EditKind::Profile(id) => {
                self.validate_draft(&EditKind::Profile(id.clone()), text)?;
                return self.save_adapter(&id, Some(Profile::parse(text)?));
            }
            EditKind::Settings => {
                let settings: Settings = toml::from_str(text)?;
                let mut cfg = self.config.clone();
                cfg.settings = settings;
                self.persist(cfg)?;
                self.log(crate::i18n::Message::new("settings-saved"));
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
                return Ok(vec![Action::Configure(Box::new(server))]);
            }
            EditKind::Manual(ids) => {
                let form: ManualForm = toml::from_str(text)?;
                let value = match (form.percent, form.raw) {
                    (Some(p), None) => ControlValue::Percent(p),
                    (None, Some(v)) => ControlValue::Raw(v),
                    _ => anyhow::bail!("specify exactly one of percent or raw"),
                };
                self.confirm_control(ids, Some((value, form.zone)))?;
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
                servers.retain(|s| !self.host_exists(&s.host));
                self.modal = Some(Modal::Import {
                    duplicates: before - servers.len(),
                    servers,
                    authenticate,
                });
            }
        }
        Ok(vec![])
    }
    pub fn model_options(&self, query: &str) -> Vec<String> {
        self.pending
            .first()
            .map(|p| {
                p.candidates
                    .iter()
                    .filter(|c| {
                        let m = &self.catalog.models[&c.id];
                        format!("{} {} {}", m.name, c.id, c.evidence.join(" "))
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                    })
                    .map(|c| c.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn resume_review(&mut self) {
        if !self.identifying && !self.pending.is_empty() && self.modal.is_none() {
            self.resolve_next();
        }
    }
    pub fn capability(
        &self,
        server: &Server,
        identity: &DeviceIdentity,
        profile: Option<&str>,
    ) -> String {
        let p = profile.and_then(|id| self.registry.profiles.get(id));
        match p {
            Some(p) if p.compatible(identity) && p.eligible(server.allow_experimental) => {
                format!("{} [{}]", self.text("manual"), p.id)
            }
            Some(p) => format!(
                "{} [{}; {}]",
                self.text("monitor-only"),
                p.id,
                if p.verification == bmcforge_profiles::Verification::Experimental {
                    self.text("experimental")
                } else {
                    self.text("unsupported")
                }
            ),
            None => self.text("monitor-only").into(),
        }
    }
    fn accept_model(&mut self, model: Option<String>) {
        let mut p = self.pending.remove(0);
        p.server.model_id = model.clone();
        if let Some(m) = model.and_then(|id| self.catalog.models.get(&id)) {
            p.server.name = m.name.clone();
            if p.server.profile.is_none() {
                p.server.profile = m.profile.clone();
            }
        }
        p.server.identity_evidence = Some(p.identity.clone());
        p.server.identification_error = p.error;
        self.snapshots.insert(
            p.server.id.clone(),
            Snapshot {
                identity: p.identity,
                ..Default::default()
            },
        );
        self.resolved.push(p.server);
    }
    fn resolve_next(&mut self) {
        loop {
            let Some(p) = self.pending.first() else {
                self.modal = Some(Modal::Import {
                    servers: std::mem::take(&mut self.resolved),
                    authenticate: false,
                    duplicates: 0,
                });
                return;
            };
            if p.error.is_none() && p.candidates.len() == 1 && p.candidates[0].explicit {
                let id = p.candidates[0].id.clone();
                self.accept_model(Some(id));
            } else {
                self.modal = Some(Modal::ModelChoice {
                    query: String::new(),
                    cursor: 0,
                });
                return;
            }
        }
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
        self.log(crate::i18n::Message::detail(
            "export",
            path.display().to_string(),
        ));
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
    fn settings_reset_is_a_draft_until_saved_and_preserves_servers() {
        let mut a = app();
        a.config.settings.theme = "light".into();
        a.config.settings.concurrency = 17;
        let servers = a.config.servers.clone();
        press(&mut a, KeyCode::Char(','));
        press(&mut a, KeyCode::F(5));
        assert_eq!(a.config.settings.theme, "light");
        press(&mut a, KeyCode::Esc);
        assert_eq!(a.config.settings.concurrency, 17);
        press(&mut a, KeyCode::Char(','));
        press(&mut a, KeyCode::F(5));
        press(&mut a, KeyCode::F(2));
        assert_eq!(a.config.settings.theme, "dark");
        assert_eq!(
            a.config.settings.concurrency,
            Settings::default().concurrency
        );
        assert_eq!(a.config.servers, servers);
    }
    #[test]
    fn adapter_edits_create_one_local_copy_migrate_references_and_reset() -> Result<()> {
        let mut a = app();
        let origin = "dell-idrac6-8";
        a.config.servers[0].profile = Some(origin.into());
        let original = a.registry.profiles[origin].clone();
        assert!(a.save_adapter(origin, Some(original.clone()))?.is_empty());
        assert!(a.registry.local.is_empty());
        let mut changed = original.clone();
        changed.notes.push_str(" local change");
        let actions = a.save_adapter(origin, Some(changed))?;
        assert!(!actions.is_empty());
        let id = a.config.servers[0].profile.clone().unwrap();
        assert_ne!(id, origin);
        assert_eq!(a.registry.local.len(), 1);
        assert_eq!(a.registry.profiles[origin].notes, original.notes);
        let mut next = a.registry.profiles[&id].clone();
        next.description.push_str(" changed");
        a.save_adapter(&id, Some(next))?;
        assert_eq!(a.registry.local.len(), 1);
        a.save_adapter(&id, None)?;
        assert!(a.registry.local.is_empty());
        assert!(!a.registry.profiles.contains_key(&id));
        assert_eq!(a.config.servers[0].profile.as_deref(), Some(origin));
        Ok(())
    }
    #[test]
    fn adapter_page_edit_works_without_servers_and_cancel_creates_nothing() {
        let mut a = app();
        a.config.servers.clear();
        a.page = 2;
        assert!(
            a.target_reason(&crate::mouse::Target::Shortcut("edit".into()))
                .is_empty()
        );
        press(&mut a, KeyCode::Char('e'));
        assert!(
            matches!(a.modal, Some(Modal::Form(ref f)) if matches!(f.kind, EditKind::Profile(_)))
        );
        press(&mut a, KeyCode::Esc);
        assert!(a.registry.local.is_empty());
    }
    #[test]
    fn adapter_save_and_reset_refuse_while_affected_device_is_taken_over() -> Result<()> {
        let mut a = app();
        let origin = "dell-idrac6-8";
        let server = a.config.servers[0].id.clone();
        a.config.servers[0].profile = Some(origin.into());
        a.snapshots.insert(
            server.clone(),
            Snapshot {
                taken_over: true,
                ..Default::default()
            },
        );
        let mut p = a.registry.profiles[origin].clone();
        p.notes.push_str(" change");
        assert!(a.save_adapter(origin, Some(p.clone())).is_err());
        assert!(a.registry.local.is_empty());
        a.snapshots.get_mut(&server).unwrap().taken_over = false;
        a.save_adapter(origin, Some(p))?;
        let id = a.config.servers[0].profile.clone().unwrap();
        a.snapshots.get_mut(&server).unwrap().taken_over = true;
        assert!(a.save_adapter(&id, None).is_err());
        assert!(a.registry.local.contains_key(&id));
        Ok(())
    }
    #[test]
    fn import_skips_existing_hosts_even_when_the_port_differs() -> Result<()> {
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
        assert!(
            matches!(&a.modal,Some(Modal::Import {servers,duplicates:1,..}) if servers.is_empty())
        );
        assert_eq!(a.config.servers.len(), 4);
        Ok(())
    }
    #[test]
    fn discovery_cannot_select_or_add_an_existing_host_at_another_port() -> Result<()> {
        let mut a = app();
        let existing = a.config.servers[0].host.clone();
        a.discovery.push(DiscoveryResult {
            host: format!("{}.", existing.to_ascii_uppercase()),
            status: ProbeStatus::IpmiCandidate,
        });
        a.modal = Some(Modal::DiscoveryResults);
        a.input(key(' '));
        assert!(a.discovery_selected.is_empty());

        let mut duplicate = a.config.servers[0].clone();
        duplicate.id = "different-id".into();
        duplicate.host = format!("{}.", existing.to_ascii_uppercase());
        duplicate.port = duplicate.port.saturating_add(1);
        assert!(a.add_servers(vec![duplicate])?.is_empty());
        assert_eq!(a.config.servers.len(), 4);
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
        assert!(matches!(a.modal, Some(Modal::DiscoveryResults)));
    }
    #[test]
    fn discovery_results_select_and_identify_from_the_dialog() {
        let mut a = app();
        a.discovery.push(DiscoveryResult {
            host: "192.0.2.20".into(),
            status: ProbeStatus::IpmiCandidate,
        });
        a.modal = Some(Modal::DiscoveryResults);
        a.input(key(' '));
        assert!(a.discovery_selected.contains("192.0.2.20"));
        a.input(key('i'));
        assert!(matches!(a.modal, Some(Modal::Form(_))));
    }
    #[test]
    fn completed_scan_waits_for_an_existing_dialog_to_close() {
        let mut a = app();
        a.scan_generation = 7;
        a.scanning = true;
        a.modal = Some(Modal::Info("keep this dialog".into()));
        a.message(Message::ScanFinished(7));
        assert!(matches!(&a.modal, Some(Modal::Info(text)) if text == "keep this dialog"));
        assert!(a.discovery_ready);
        a.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(a.modal, Some(Modal::DiscoveryResults)));
        assert!(!a.discovery_ready);
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
        assert!(matches!(a.modal, Some(Modal::Form(_))));
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
    #[test]
    fn identification_waits_for_resolution_and_final_confirmation_and_ignores_late_results() {
        let mut a = app();
        let original = a.config.servers.len();
        a.identify_generation = 8;
        a.identifying = true;
        let mut s = bmcforge_ipmi::mock::servers().remove(2);
        s.id = "new".into();
        s.host = "192.0.2.20".into();
        let identity = DeviceIdentity {
            manufacturer_id: 19046,
            model: "IBM System x3650 M5".into(),
            machine_types: vec!["5463-ABC".into()],
            ..Default::default()
        };
        a.message(Message::Identified(7, s.clone(), identity.clone(), None));
        assert!(a.pending.is_empty());
        a.message(Message::Identified(8, s, identity.clone(), None));
        assert_eq!(a.pending[0].candidates.len(), 2);
        assert_eq!(a.config.servers.len(), original);
        a.message(Message::IdentificationFinished(8));
        assert!(matches!(a.modal, Some(Modal::ModelChoice { .. })));
        if let Some(Modal::ModelChoice { query, .. }) = &mut a.modal {
            *query = "3650".into();
        }
        a.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            a.modal,
            Some(Modal::Import {
                authenticate: false,
                ..
            })
        ));
        assert_eq!(a.config.servers.len(), original);
        a.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let saved = a.config.servers.last().unwrap();
        assert_eq!(saved.model_id.as_deref(), Some("lenovo-x3650-m5"));
        assert_eq!(
            saved.identity_evidence.as_ref().unwrap().machine_types,
            identity.machine_types
        );
        assert!(
            !a.registry.profiles["lenovo-x3650-m5-imm2"]
                .compatible(saved.identity_evidence.as_ref().unwrap())
        );
        a.message(Message::Identified(8, saved.clone(), identity, None));
        assert!(a.pending.is_empty());
    }
    #[test]
    fn failed_authentication_and_no_fru_are_never_guessed() {
        let mut a = app();
        a.identifying = true;
        a.identify_generation = 1;
        let s = bmcforge_ipmi::mock::servers().remove(0);
        a.message(Message::Identified(
            1,
            s,
            DeviceIdentity {
                manufacturer_id: 674,
                ..Default::default()
            },
            Some("credential rejected".into()),
        ));
        a.message(Message::IdentificationFinished(1));
        assert!(a.model_options("").is_empty());
        assert!(matches!(a.modal, Some(Modal::ModelChoice { .. })));
        a.input(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
        assert!(matches!(a.modal,Some(Modal::Import{ref servers,..}) if servers.is_empty()));
    }
    fn press(app: &mut App, code: KeyCode) -> Vec<Action> {
        app.input(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn profile_enter_opens_full_details_and_escape_closes_them() {
        let mut app = app();
        app.page = 2;
        app.profile_cursor = 0;
        let selected_id = app.registry.profiles.keys().next().unwrap().clone();

        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            &app.modal,
            Some(Modal::ProfileDetail { id, reveal: false }) if id == &selected_id
        ));
        assert_eq!(app.modal_scroll, 0);

        press(&mut app, KeyCode::PageDown);
        assert!(app.modal_scroll > 0);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
    }
    fn set_field(app: &mut App, path: &[&str], value: &str) {
        let Some(Modal::Form(form)) = &mut app.modal else {
            panic!("expected form");
        };
        let index = form
            .fields
            .iter()
            .position(|f| f.path.iter().map(String::as_str).eq(path.iter().copied()))
            .unwrap();
        form.fields[index].input = Editor::new(value);
        form.focus = index;
    }
    #[test]
    fn prompt_import_credentials_and_confirmation_need_no_toml() {
        let mut a = app();
        let before = a.config.servers.len();
        press(&mut a, KeyCode::Char('i'));
        a.paste("192.0.2.240\n192.0.2.241");
        assert!(press(&mut a, KeyCode::F(2)).is_empty());
        assert!(matches!(a.modal, Some(Modal::Preview { .. })));
        press(&mut a, KeyCode::Enter);
        set_field(&mut a, &["username"], "operator");
        set_field(&mut a, &["password"], "密钥");
        set_field(&mut a, &["port"], "624");
        press(&mut a, KeyCode::F(2));
        assert!(matches!(a.modal, Some(Modal::Import { .. })));
        assert_eq!(a.config.servers.len(), before);
        let actions = press(&mut a, KeyCode::Enter);
        assert!(
            matches!(actions.as_slice(), [Action::Add(s)] if s.len() == 2 && s.iter().all(|s| s.port == 624 && s.credentials.password == "密钥"))
        );
        assert_eq!(a.config.servers.len(), before + 2);
    }
    #[test]
    fn settings_errors_cancel_and_advanced_roundtrip_preserve_drafts() {
        let mut a = app();
        let before = toml::to_string(&a.config).unwrap();
        press(&mut a, KeyCode::Char(','));
        set_field(&mut a, &["scan_port"], "0");
        assert!(press(&mut a, KeyCode::F(2)).is_empty());
        assert!(
            matches!(&a.modal, Some(Modal::Form(f)) if !f.error.is_empty() && f.fields[f.focus].path[0] == "scan_port")
        );
        assert_eq!(toml::to_string(&a.config).unwrap(), before);
        set_field(&mut a, &["scan_port"], "624");
        press(&mut a, KeyCode::F(7));
        let Some(Modal::Editor { editor, .. }) = &mut a.modal else {
            panic!("advanced editor");
        };
        *editor = Editor::new("scan_port = 0");
        press(&mut a, KeyCode::F(7));
        assert!(
            matches!(&a.modal, Some(Modal::Editor { error, editor, .. }) if !error.is_empty() && editor.text() == "scan_port = 0")
        );
        let Some(Modal::Editor { editor, .. }) = &mut a.modal else {
            unreachable!()
        };
        let mut settings = a.config.settings.clone();
        settings.scan_port = 624;
        *editor = Editor::new(&toml::to_string(&settings).unwrap());
        press(&mut a, KeyCode::F(7));
        assert!(
            matches!(&a.modal, Some(Modal::Form(f)) if f.fields.iter().any(|f| f.path[0] == "scan_port" && f.value() == "624"))
        );
        assert_eq!(a.config.settings.scan_port, 623);
        press(&mut a, KeyCode::Esc);
        assert_eq!(toml::to_string(&a.config).unwrap(), before);
        press(&mut a, KeyCode::Char(','));
        set_field(&mut a, &["keys", "quit"], "Q");
        press(&mut a, KeyCode::F(2));
        assert!(a.modal.is_none());
        assert!(matches!(
            a.input(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT))
                .as_slice(),
            [Action::Quit]
        ));
    }
    #[test]
    fn searchable_choice_and_server_form_wait_for_worker_ack() {
        let mut a = app();
        let id = a.current().unwrap().id.clone();
        let old = a.current().unwrap().name.clone();
        press(&mut a, KeyCode::Char('e'));
        set_field(&mut a, &["name"], "新名称");
        let Some(Modal::Form(form)) = &mut a.modal else {
            unreachable!()
        };
        form.focus = form
            .fields
            .iter()
            .position(|f| f.path[0] == "profile")
            .unwrap();
        press(&mut a, KeyCode::Enter);
        assert!(matches!(a.modal, Some(Modal::FieldChoice { .. })));
        a.paste("supermicro");
        if let Some(Modal::FieldChoice { form, query, .. }) = &a.modal {
            assert!(!form.choice_options(query).is_empty());
        }
        press(&mut a, KeyCode::Esc);
        assert!(
            matches!(&a.modal, Some(Modal::Form(f)) if f.fields.iter().any(|f| f.path[0] == "name" && f.value() == "新名称"))
        );
        let actions = press(&mut a, KeyCode::F(2));
        assert!(
            matches!(actions.as_slice(), [Action::Configure(s)] if s.id == id && s.name == "新名称")
        );
        assert_eq!(a.current().unwrap().name, old);
    }
    #[test]
    fn minimal_advanced_settings_expand_and_unbound_actions_still_work() {
        let mut a = app();
        a.config.settings.keys.remove("import");
        press(&mut a, KeyCode::Char(','));
        let Some(Modal::Form(f)) = &a.modal else {
            unreachable!()
        };
        assert!(
            f.fields
                .iter()
                .any(|f| f.path == ["keys", "import"] && f.value().is_empty())
        );
        press(&mut a, KeyCode::F(7));
        let Some(Modal::Editor { editor, .. }) = &mut a.modal else {
            unreachable!()
        };
        *editor = Editor::new("");
        press(&mut a, KeyCode::F(7));
        let Some(Modal::Form(f)) = &a.modal else {
            unreachable!()
        };
        assert!(f.fields.iter().any(|f| f.path[0] == "theme"));
        // Stale background actions cannot bypass a modal.
        assert!(
            a.invoke_target(crate::mouse::Target::Shortcut("quit".into()))
                .is_empty()
        );
        press(&mut a, KeyCode::Esc);
        assert!(
            a.invoke_target(crate::mouse::Target::Shortcut("import".into()))
                .is_empty()
        );
        assert!(matches!(a.modal, Some(Modal::Form(_))));
    }
    #[test]
    fn manual_forms_support_percentage_raw_and_mixed_batch_validation() {
        let mut a = app();
        for id in ["demo-dell", "demo-smc", "demo-lenovo"] {
            a.selected = BTreeSet::from([id.into()]);
            press(&mut a, KeyCode::Char('m'));
            assert!(matches!(a.modal, Some(Modal::Form(_))));
            assert!(press(&mut a, KeyCode::F(2)).is_empty());
            assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
            let actions = press(&mut a, KeyCode::Enter);
            assert!(matches!(
                actions.as_slice(),
                [Action::Send(_, Command::ManualValue(_, _, _))]
            ));
        }
        a.selected = BTreeSet::from(["demo-dell".into(), "demo-lenovo".into()]);
        press(&mut a, KeyCode::Char('m'));
        assert!(press(&mut a, KeyCode::F(2)).is_empty());
        assert!(matches!(&a.modal, Some(Modal::Form(f)) if !f.error.is_empty()));
        press(&mut a, KeyCode::Esc);
        a.page = 2;
        let (index, note) = a
            .registry
            .profiles
            .values()
            .enumerate()
            .find_map(|(i, p)| p.default_credentials.first().map(|c| (i, c.note.clone())))
            .unwrap();
        a.profile_cursor = index;
        let before = a.current().unwrap().credentials.clone();
        press(&mut a, KeyCode::Char('u'));
        assert!(matches!(&a.modal, Some(Modal::Form(f)) if f.title.contains(&note)));
        press(&mut a, KeyCode::Esc);
        assert_eq!(a.current().unwrap().credentials, before);
    }
}

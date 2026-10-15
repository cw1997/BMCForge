use crate::{app::EditKind, editor::Editor};
use anyhow::{Context, Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Debug)]
pub enum FieldType {
    Text,
    Multiline,
    Secret,
    Number { min: i64, max: i64 },
    Toggle,
    Choice(Vec<String>),
    Tags,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub path: Vec<String>,
    pub label: String,
    pub group: &'static str,
    pub input: Editor,
    pub kind: FieldType,
    pub optional: bool,
    pub revealed: bool,
    pub error: String,
}
impl Field {
    pub fn value(&self) -> String {
        self.input.text()
    }
    pub fn display(&self) -> String {
        if matches!(self.kind, FieldType::Secret) && !self.revealed {
            "•".repeat(self.value().chars().count())
        } else {
            self.value()
        }
    }
    pub fn activate(&mut self, backwards: bool) {
        let value = self.value();
        let next = match &self.kind {
            FieldType::Toggle => {
                if value == "true" {
                    "false".into()
                } else {
                    "true".into()
                }
            }
            FieldType::Choice(options) if !options.is_empty() => {
                let index = options.iter().position(|v| v == &value).unwrap_or(0);
                options[(index + if backwards { options.len() - 1 } else { 1 }) % options.len()]
                    .clone()
            }
            _ => return,
        };
        self.input = Editor::new(&next);
        self.error.clear();
    }
}

#[derive(Clone)]
pub struct Form {
    pub title: String,
    pub kind: EditKind,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub error: String,
    pub base: toml::Table,
    pub advanced: bool,
}

fn field(path: Vec<String>, value: &toml::Value) -> Field {
    let key = path.last().unwrap().as_str();
    let optional =
        matches!(key, "profile" | "model_id" | "kg") || path.first().is_some_and(|p| p == "keys");
    let kind = match key {
        "password" | "kg" => FieldType::Secret,
        "theme" => FieldType::Choice(vec!["dark".into(), "light".into()]),
        "temperature_unit" => FieldType::Choice(vec!["C".into(), "F".into()]),
        "port" | "scan_port" => FieldType::Number { min: 1, max: 65535 },
        "timeout_ms"
        | "poll_seconds"
        | "scan_timeout_ms"
        | "scan_confirm_above"
        | "scan_rate_per_second"
        | "history_capacity" => FieldType::Number {
            min: 1,
            max: u32::MAX.into(),
        },
        "concurrency" | "scan_concurrency" => FieldType::Number { min: 1, max: 1024 },
        "retries" | "scan_retries" => FieldType::Number { min: 0, max: 10 },
        "privilege" => FieldType::Choice((1..=5).map(|v| v.to_string()).collect()),
        "percent" => FieldType::Number { min: 0, max: 100 },
        "raw" | "zone" => FieldType::Number { min: 0, max: 255 },
        _ => match value {
            toml::Value::Boolean(_) => FieldType::Toggle,
            toml::Value::Array(_) => FieldType::Tags,
            toml::Value::Integer(_) => FieldType::Number {
                min: 0,
                max: u32::MAX.into(),
            },
            _ => FieldType::Text,
        },
    };
    let group = if path.first().is_some_and(|p| p == "keys") {
        "form-shortcuts"
    } else if path.first().is_some_and(|p| p == "credentials")
        || matches!(key, "username" | "password" | "kg")
    {
        "credentials"
    } else if key.starts_with("scan_") {
        "discovery"
    } else if matches!(key, "theme" | "language" | "temperature_unit" | "mouse") {
        "form-appearance"
    } else if matches!(
        key,
        "timeout_ms"
            | "retries"
            | "poll_seconds"
            | "privilege"
            | "concurrency"
            | "history_capacity"
    ) {
        "form-runtime"
    } else {
        "form-basic"
    };
    let text = match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str())
            .map(|v| {
                if v.contains([',', '"', '\n']) || v.trim() != v {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => value.to_string(),
    };
    Field {
        label: if path.first().is_some_and(|p| p == "keys") {
            key.into()
        } else {
            format!("field-{}", key.replace('_', "-"))
        },
        path,
        group,
        input: Editor::new(&text),
        kind,
        optional,
        revealed: false,
        error: String::new(),
    }
}

impl Form {
    pub fn new(title: String, kind: EditKind, text: &str) -> Result<Self> {
        let advanced = !matches!(kind, EditKind::Search | EditKind::Targets(_));
        let mut base = if advanced {
            toml::from_str::<toml::Table>(text)?
        } else {
            toml::Table::new()
        };
        // Expand serde defaults when returning from a deliberately minimal advanced draft.
        match &kind {
            EditKind::Settings => {
                base = toml::Value::try_from(toml::from_str::<bmcforge_core::config::Settings>(
                    text,
                )?)?
                .as_table()
                .unwrap()
                .clone();
            }
            EditKind::Server(_) => {
                base =
                    toml::Value::try_from(toml::from_str::<bmcforge_core::config::Server>(text)?)?
                        .as_table()
                        .unwrap()
                        .clone();
            }
            _ => {}
        }
        let mut fields = vec![];
        if !advanced {
            let mut f = field(vec!["input".into()], &toml::Value::String(text.into()));
            f.label = if matches!(kind, EditKind::Search) {
                "search"
            } else {
                "field-targets"
            }
            .into();
            if matches!(kind, EditKind::Targets(_)) {
                f.kind = FieldType::Multiline;
            }
            fields.push(f);
        } else {
            if matches!(kind, EditKind::Settings) {
                let keys = base
                    .entry("keys")
                    .or_insert(toml::Value::Table(Default::default()))
                    .as_table_mut()
                    .unwrap();
                for action in crate::interaction::ACTIONS {
                    keys.entry(*action)
                        .or_insert(toml::Value::String(String::new()));
                }
            }
            // Missing optional values still need editable controls. Keep evidence and IDs in base only.
            if matches!(kind, EditKind::Server(_) | EditKind::Credentials(..)) {
                base.entry("profile")
                    .or_insert(toml::Value::String(String::new()));
            }
            if matches!(kind, EditKind::Server(_)) {
                base.entry("kg")
                    .or_insert(toml::Value::String(String::new()));
            }
            fn visit(table: &toml::Table, prefix: Vec<String>, fields: &mut Vec<Field>) {
                for (key, value) in table {
                    if prefix.is_empty()
                        && matches!(
                            key.as_str(),
                            "id" | "identity_evidence" | "identification_error" | "model_id"
                        )
                    {
                        continue;
                    }
                    let mut path = prefix.clone();
                    path.push(key.clone());
                    if let toml::Value::Table(t) = value {
                        visit(t, path, fields);
                    } else {
                        fields.push(field(path, value));
                    }
                }
            }
            visit(&base, vec![], &mut fields);
            fields.sort_by_key(|f| match f.group {
                "form-basic" => 0,
                "form-appearance" => 1,
                "credentials" => 2,
                "discovery" => 3,
                "form-runtime" => 4,
                _ => 5,
            });
        }
        ensure!(!fields.is_empty(), "empty form");
        Ok(Self {
            title,
            kind,
            fields,
            focus: 0,
            error: String::new(),
            base,
            advanced,
        })
    }
    pub fn input(&mut self, key: KeyEvent) {
        self.error.clear();
        match key.code {
            KeyCode::Tab | KeyCode::Down
                if !matches!(self.fields[self.focus].kind, FieldType::Multiline)
                    || key.code == KeyCode::Tab =>
            {
                self.focus = (self.focus + 1) % self.fields.len()
            }
            KeyCode::BackTab | KeyCode::Up
                if !matches!(self.fields[self.focus].kind, FieldType::Multiline)
                    || key.code == KeyCode::BackTab =>
            {
                self.focus = (self.focus + self.fields.len() - 1) % self.fields.len()
            }
            KeyCode::F(6) => self.fields[self.focus].revealed = !self.fields[self.focus].revealed,
            _ => {
                let f = &mut self.fields[self.focus];
                match &f.kind {
                    FieldType::Toggle | FieldType::Choice(_) => {
                        if matches!(
                            key.code,
                            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                        ) {
                            f.activate(key.code == KeyCode::Left);
                        }
                    }
                    _ => {
                        if key.code != KeyCode::Enter || matches!(f.kind, FieldType::Multiline) {
                            f.input.input(key);
                            f.error.clear();
                        }
                    }
                }
            }
        }
    }
    pub fn paste(&mut self, text: &str) {
        self.error.clear();
        let f = &mut self.fields[self.focus];
        if matches!(f.kind, FieldType::Toggle | FieldType::Choice(_)) {
            return;
        }
        if matches!(f.kind, FieldType::Multiline) {
            f.input.paste(text);
        } else {
            f.input.paste(&text.replace(['\r', '\n'], ""));
        }
        f.error.clear();
    }
    pub fn text(&mut self) -> Result<String> {
        if !self.advanced {
            return Ok(self.fields[0].value());
        }
        let mut table = self.base.clone();
        let mut shortcuts = std::collections::BTreeSet::new();
        for (i, f) in self.fields.iter_mut().enumerate() {
            let text = f.value();
            let result: Result<toml::Value> = (|| {
                if f.path.first().is_some_and(|p| p == "keys") && !text.is_empty() {
                    ensure!(
                        text.chars().count() == 1
                            && !"12345".contains(&text)
                            && shortcuts.insert(text.clone()),
                        "form-error-key"
                    );
                }
                match f.path.last().unwrap().as_str() {
                    "username" => ensure!(text.len() <= 16, "form-error-username"),
                    "password" | "kg" => ensure!(text.len() <= 20, "form-error-secret"),
                    "host" => ensure!(!text.trim().is_empty(), "form-error-required"),
                    _ => {}
                }
                Ok(match &f.kind {
                    FieldType::Number { min, max } => {
                        let n: i64 = if let Some(hex) = text.trim().strip_prefix("0x") {
                            i64::from_str_radix(hex, 16)
                        } else {
                            text.trim().parse()
                        }
                        .context("form-error-number")?;
                        ensure!((*min..=*max).contains(&n), "form-error-range");
                        toml::Value::Integer(n)
                    }
                    FieldType::Toggle => toml::Value::Boolean(text == "true"),
                    FieldType::Tags => toml::Value::Array(
                        parse_tags(&text)?
                            .into_iter()
                            .map(toml::Value::String)
                            .collect(),
                    ),
                    FieldType::Choice(options) => {
                        ensure!(options.contains(&text), "form-error-choice");
                        if f.path.last().is_some_and(|p| {
                            matches!(p.as_str(), "privilege" | "percent" | "raw" | "zone")
                        }) {
                            toml::Value::Integer(text.parse()?)
                        } else {
                            toml::Value::String(text.clone())
                        }
                    }
                    _ => toml::Value::String(text.clone()),
                })
            })();
            let value = match result {
                Ok(v) => v,
                Err(e) => {
                    f.error = e.to_string();
                    self.focus = i;
                    bail!("{}", f.error);
                }
            };
            let mut parent = &mut table;
            for p in &f.path[..f.path.len() - 1] {
                parent = parent.get_mut(p).unwrap().as_table_mut().unwrap();
            }
            let key = f.path.last().unwrap();
            if f.optional && text.is_empty() {
                parent.remove(key);
            } else {
                parent.insert(key.clone(), value);
            }
        }
        Ok(toml::to_string_pretty(&table)?)
    }
    pub fn choice_options(&self, query: &str) -> Vec<String> {
        if let FieldType::Choice(options) = &self.fields[self.focus].kind {
            options
                .iter()
                .filter(|v| v.to_lowercase().contains(&query.to_lowercase()))
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }
    pub fn locate_error(&mut self, error: &str) {
        if let Some(i) = self
            .fields
            .iter()
            .position(|f| f.path.last().is_some_and(|p| error.contains(p)))
        {
            self.focus = i;
        }
        self.error = error.into();
        self.fields[self.focus].error = error.into();
    }
}

// Quoted comma-separated tags preserve existing labels containing punctuation or spaces.
fn parse_tags(text: &str) -> Result<Vec<String>> {
    let mut tags = vec![];
    let mut value = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut was_quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                value.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if value.trim().is_empty() => {
                value.clear();
                quoted = true;
                was_quoted = true;
            }
            ',' if !quoted => {
                let tag = if was_quoted {
                    value.clone()
                } else {
                    value.trim().to_owned()
                };
                if !tag.is_empty() {
                    tags.push(tag);
                }
                value.clear();
                was_quoted = false;
            }
            c if !quoted && was_quoted && c.is_whitespace() => {}
            c => value.push(c),
        }
    }
    ensure!(!quoted, "form-error-tags");
    let tag = if was_quoted {
        value
    } else {
        value.trim().to_owned()
    };
    if !tag.is_empty() {
        tags.push(tag);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmcforge_core::config::{Server, Settings};
    use crossterm::event::KeyModifiers;

    #[test]
    fn server_roundtrip_preserves_identity_secrets_and_quoted_tags() {
        let server = Server {
            id: "immutable".into(),
            host: "192.0.2.10".into(),
            model_id: Some("device-model".into()),
            identity_evidence: Some(Default::default()),
            kg: Some("私密".into()),
            tags: vec!["rack,a".into(), " quoted \"tag\" ".into()],
            ..Default::default()
        };
        let mut form = Form::new(
            "Server".into(),
            EditKind::Server(server.id.clone()),
            &toml::to_string(&server).unwrap(),
        )
        .unwrap();
        assert!(
            !form
                .fields
                .iter()
                .any(|f| matches!(f.path[0].as_str(), "id" | "model_id" | "identity_evidence"))
        );
        let secret = form.fields.iter_mut().find(|f| f.path[0] == "kg").unwrap();
        assert_eq!(secret.display(), "••");
        secret.revealed = true;
        assert_eq!(secret.display(), "私密");
        let result: Server = toml::from_str(&form.text().unwrap()).unwrap();
        assert_eq!(result, server);
    }

    #[test]
    fn invalid_numbers_and_credentials_keep_draft_and_focus_error() {
        let mut form = Form::new(
            "Settings".into(),
            EditKind::Settings,
            &toml::to_string(&Settings::default()).unwrap(),
        )
        .unwrap();
        let port = form
            .fields
            .iter()
            .position(|f| f.path[0] == "scan_port")
            .unwrap();
        form.fields[port].input = Editor::new("65536");
        assert!(form.text().is_err());
        assert_eq!(form.focus, port);
        assert_eq!(form.fields[port].value(), "65536");
        form.fields[port].input = Editor::new("0x26f");
        let settings: Settings = toml::from_str(&form.text().unwrap()).unwrap();
        assert_eq!(settings.scan_port, 623);
        let server = Server {
            id: "s".into(),
            host: "192.0.2.1".into(),
            ..Default::default()
        };
        let mut form = Form::new(
            "Server".into(),
            EditKind::Server(server.id.clone()),
            &toml::to_string(&server).unwrap(),
        )
        .unwrap();
        let username = form
            .fields
            .iter()
            .position(|f| f.path.last().unwrap() == "username")
            .unwrap();
        form.fields[username].input = Editor::new("服务器服务器");
        assert!(form.text().is_err());
        assert_eq!(form.focus, username);
    }

    #[test]
    fn unicode_prompts_newlines_focus_and_duplicate_shortcuts() {
        let mut form = Form::new("Targets".into(), EditKind::Targets(false), "").unwrap();
        form.paste("服务器\r\n192.0.2.1");
        assert_eq!(form.text().unwrap(), "服务器\n192.0.2.1");
        let mut search = Form::new("Search".into(), EditKind::Search, "").unwrap();
        search.paste("服务器\r\nA");
        assert_eq!(search.text().unwrap(), "服务器A");
        let mut settings = Form::new(
            "Settings".into(),
            EditKind::Settings,
            &toml::to_string(&Settings::default()).unwrap(),
        )
        .unwrap();
        settings.input(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(settings.focus, settings.fields.len() - 1);
        settings.input(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(settings.focus, 0);
        let quit = settings
            .fields
            .iter()
            .position(|f| f.path == ["keys", "quit"])
            .unwrap();
        settings.fields[quit].input = Editor::new("i");
        assert!(settings.text().is_err());
        assert_eq!(settings.focus, quit);
    }
}

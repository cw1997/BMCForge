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
    Structured,
    Collection,
    ReadOnly,
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
    pub show_descriptions: bool,
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
            | "sensor_filter"
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
            EditKind::Profile(_) => {
                base = toml::Value::try_from(toml::from_str::<bmcforge_profiles::Profile>(text)?)?
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
            if matches!(kind, EditKind::Profile(_)) {
                fields.clear();
                profile_fields(&toml::Value::Table(base.clone()), vec![], &mut fields);
                for path in [
                    vec!["matcher", "manufacturer_id"],
                    vec!["matcher", "firmware_min"],
                    vec!["matcher", "firmware_max"],
                    vec!["fan"],
                ] {
                    let path: Vec<String> = path.into_iter().map(str::to_owned).collect();
                    let mut node = &toml::Value::Table(base.clone());
                    let found = path.iter().all(|p| {
                        if let Some(next) = node.get(p) {
                            node = next;
                            true
                        } else {
                            false
                        }
                    });
                    if !found {
                        let mut f = field(path.clone(), &toml::Value::String(String::new()));
                        f.label = path.join(".");
                        f.kind = if path.last().is_some_and(|p| p == "manufacturer_id") {
                            FieldType::Structured
                        } else if path == ["fan"] {
                            FieldType::Structured
                        } else {
                            FieldType::Text
                        };
                        f.optional = true;
                        f.group = if path.first().is_some_and(|p| p == "matcher") {
                            "adapter-matching"
                        } else {
                            "adapter-control"
                        };
                        fields.push(f);
                    }
                }
            }
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
            show_descriptions: false,
        })
    }
    pub fn input(&mut self, key: KeyEvent) {
        if matches!(self.kind, EditKind::Profile(_))
            && (matches!(
                key.code,
                KeyCode::F(4) | KeyCode::F(5) | KeyCode::F(9) | KeyCode::F(10)
            ) || (key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Up | KeyCode::Down)))
        {
            if let Err(error) = self.edit_collection(key) {
                self.locate_error(&error.to_string());
            }
            return;
        }
        self.error.clear();
        match key.code {
            KeyCode::Tab | KeyCode::Down
                if !matches!(
                    self.fields[self.focus].kind,
                    FieldType::Multiline | FieldType::Structured
                ) || key.code == KeyCode::Tab =>
            {
                self.focus = (self.focus + 1) % self.fields.len()
            }
            KeyCode::BackTab | KeyCode::Up
                if !matches!(
                    self.fields[self.focus].kind,
                    FieldType::Multiline | FieldType::Structured
                ) || key.code == KeyCode::BackTab =>
            {
                self.focus = (self.focus + self.fields.len() - 1) % self.fields.len()
            }
            KeyCode::F(6) => self.fields[self.focus].revealed = !self.fields[self.focus].revealed,
            _ => {
                let f = &mut self.fields[self.focus];
                match &f.kind {
                    FieldType::Collection | FieldType::ReadOnly => {}
                    FieldType::Toggle | FieldType::Choice(_) => {
                        if matches!(
                            key.code,
                            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                        ) {
                            f.activate(key.code == KeyCode::Left);
                        }
                    }
                    _ => {
                        if key.code != KeyCode::Enter
                            || matches!(f.kind, FieldType::Multiline | FieldType::Structured)
                        {
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
        if matches!(
            f.kind,
            FieldType::Toggle | FieldType::Choice(_) | FieldType::Collection | FieldType::ReadOnly
        ) {
            return;
        }
        if matches!(f.kind, FieldType::Multiline | FieldType::Structured) {
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
            if matches!(f.kind, FieldType::Collection | FieldType::ReadOnly) {
                continue;
            }
            let text = f.value();
            if f.optional && text.is_empty() {
                let mut root = toml::Value::Table(table);
                set_path(&mut root, &f.path, None)?;
                table = root.as_table().unwrap().clone();
                continue;
            }
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
                    FieldType::Structured => {
                        if text.trim().is_empty() && f.optional {
                            toml::Value::String(String::new())
                        } else {
                            let wrapper: toml::Table = toml::from_str(&format!("value = {text}"))?;
                            wrapper["value"].clone()
                        }
                    }
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
            let mut root = toml::Value::Table(table);
            set_path(
                &mut root,
                &f.path,
                if f.optional && text.is_empty() {
                    None
                } else {
                    Some(value)
                },
            )?;
            table = root.as_table().unwrap().clone();
        }
        Ok(toml::to_string_pretty(&table)?)
    }
    fn edit_collection(&mut self, key: KeyEvent) -> Result<()> {
        let field_path = self.fields[self.focus].path.clone();
        let focus = self.focus;
        let mut root: toml::Value = toml::Value::Table(toml::from_str(&self.text()?)?);
        if self.fields[focus].optional && key.code == KeyCode::F(5) {
            set_path(&mut root, &field_path, None)?;
            *self = Self::new(
                self.title.clone(),
                self.kind.clone(),
                &toml::to_string_pretty(&root)?,
            )?;
            self.focus = focus.min(self.fields.len() - 1);
            return Ok(());
        }
        if self.fields[focus].optional
            && self.fields[focus].value().is_empty()
            && key.code == KeyCode::F(4)
            && field_path != ["fan"]
        {
            let value = match field_path.last().map(String::as_str) {
                Some("expect" | "expect_data") => toml::Value::Array(vec![]),
                Some("manufacturer_id" | "raw") => toml::Value::Integer(0),
                _ => toml::Value::String(String::new()),
            };
            set_path(&mut root, &field_path, Some(value))?;
            *self = Self::new(
                self.title.clone(),
                self.kind.clone(),
                &toml::to_string_pretty(&root)?,
            )?;
            self.focus = focus.min(self.fields.len() - 1);
            return Ok(());
        }
        let mut next_path = field_path.clone();
        let mut array_path = None;
        for len in (1..=field_path.len()).rev() {
            if node_mut(&mut root, &field_path[..len]).is_ok_and(|v| v.is_array()) {
                array_path = Some(field_path[..len].to_vec());
                break;
            }
        }
        if let Some(path) = array_path {
            let array = node_mut(&mut root, &path)?.as_array_mut().unwrap();
            let index = field_path
                .get(path.len())
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or_else(|| array.len().saturating_sub(1));
            match key.code {
                KeyCode::F(4) => {
                    let value = item_template(&path)?;
                    let insert = if array.is_empty() {
                        0
                    } else {
                        (index + 1).min(array.len())
                    };
                    array.insert(insert, value);
                    next_path = path.clone();
                    next_path.push(insert.to_string());
                }
                KeyCode::F(5) if !array.is_empty() => {
                    array.remove(index);
                }
                KeyCode::Up | KeyCode::F(9) if index > 0 => {
                    array.swap(index, index - 1);
                    if next_path.len() > path.len() {
                        next_path[path.len()] = (index - 1).to_string();
                    }
                }
                KeyCode::Down | KeyCode::F(10) if index + 1 < array.len() => {
                    array.swap(index, index + 1);
                    if next_path.len() > path.len() {
                        next_path[path.len()] = (index + 1).to_string();
                    }
                }
                _ => {}
            }
        } else if self.fields[focus].optional {
            if key.code == KeyCode::F(5) {
                set_path(&mut root, &field_path, None)?;
            } else if field_path == ["fan"] && root.get("fan").is_none() {
                let fan: toml::Table = toml::from_str(
                    "min_percent=0\nmax_percent=100\nscale=100\nzones=[]\nmanual=[]\nset=[]\nautomatic=[]",
                )?;
                set_path(&mut root, &field_path, Some(toml::Value::Table(fan)))?;
            }
        } else {
            bail!("select a list or optional field");
        }
        *self = Self::new(
            self.title.clone(),
            self.kind.clone(),
            &toml::to_string_pretty(&root)?,
        )?;
        self.focus = self
            .fields
            .iter()
            .position(|f| f.path.starts_with(&next_path))
            .unwrap_or_else(|| focus.min(self.fields.len() - 1));
        Ok(())
    }
    pub fn reset_settings(&mut self, all: bool) -> Result<()> {
        let defaults = Self::new(
            self.title.clone(),
            self.kind.clone(),
            &toml::to_string_pretty(&bmcforge_core::config::Settings::default())?,
        )?;
        let path = self.fields[self.focus].path.clone();
        if all {
            *self = defaults;
        } else {
            let group = self.fields[self.focus].group;
            self.fields.retain(|f| f.group != group);
            for field in defaults.fields.into_iter().filter(|f| f.group == group) {
                self.fields.push(field);
            }
            for (key, value) in defaults.base {
                if field(vec![key.clone()], &value).group == group {
                    self.base.insert(key, value);
                }
            }
            self.fields.sort_by_key(|f| match f.group {
                "form-basic" => 0,
                "form-appearance" => 1,
                "credentials" => 2,
                "discovery" => 3,
                "form-runtime" => 4,
                _ => 5,
            });
            self.error.clear();
        }
        self.focus = self.fields.iter().position(|f| f.path == path).unwrap_or(0);
        Ok(())
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

// Profiles use indexed paths so each nested list item has independently typed fields.
fn profile_fields(value: &toml::Value, path: Vec<String>, fields: &mut Vec<Field>) {
    let group = match path.first().map(String::as_str) {
        Some("matcher") => "adapter-matching",
        Some("fan") => "adapter-control",
        Some("examples") => "adapter-examples",
        Some("default_credentials") => "credentials",
        _ => "form-basic",
    };
    match value {
        toml::Value::Table(table) => {
            if path == ["fan"] {
                let mut f = field(path.clone(), value);
                f.label = "fan".into();
                f.group = group;
                f.optional = true;
                f.kind = FieldType::Collection;
                f.input = Editor::new("…");
                fields.push(f);
            }
            for (key, value) in table {
                let mut next = path.clone();
                next.push(key.clone());
                profile_fields(value, next, fields);
            }
            let optional: &[&str] = if path.first().is_some_and(|p| p == "default_credentials") {
                &["password"]
            } else if path.last().is_some_and(|p| p.parse::<usize>().is_ok())
                && path.first().is_some_and(|p| p == "fan")
                && path.len() == 3
                && path[1] != "zones"
            {
                &["expect", "expect_data"]
            } else if path.first().is_some_and(|p| p == "examples") {
                &["raw"]
            } else {
                &[]
            };
            for key in optional {
                if !table.contains_key(*key) {
                    let mut next = path.clone();
                    next.push((*key).into());
                    let mut f = field(next.clone(), &toml::Value::String(String::new()));
                    f.label = next.join(".");
                    f.group = group;
                    f.optional = true;
                    f.kind = if *key == "password" {
                        FieldType::Secret
                    } else {
                        FieldType::Structured
                    };
                    fields.push(f);
                }
            }
        }
        toml::Value::Array(items) => {
            let mut f = field(path.clone(), value);
            f.label = path.join(".");
            f.group = group;
            f.kind = FieldType::Collection;
            f.optional = path
                .last()
                .is_some_and(|p| matches!(p.as_str(), "expect" | "expect_data"));
            f.input = Editor::new(&format!("[{}]", items.len()));
            fields.push(f);
            for (index, item) in items.iter().enumerate() {
                let mut next = path.clone();
                next.push(index.to_string());
                profile_fields(item, next, fields);
            }
        }
        _ => {
            let mut f = field(path.clone(), value);
            f.label = path.join(".");
            f.group = group;
            if path.last().is_some_and(|p| {
                matches!(
                    p.as_str(),
                    "manufacturer_id" | "firmware_min" | "firmware_max" | "raw" | "password"
                )
            }) {
                f.optional = true;
            }
            if path
                .last()
                .is_some_and(|p| matches!(p.as_str(), "description" | "notes" | "note"))
            {
                f.kind = FieldType::Multiline;
            }
            if path.last().is_some_and(|p| p == "verification") {
                f.kind = FieldType::Choice(
                    [
                        "hardware_verified",
                        "documented",
                        "experimental",
                        "unsupported",
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                );
            }
            if path.len() == 1
                && matches!(
                    path[0].as_str(),
                    "id" | "schema_version" | "override_builtin"
                )
            {
                f.kind = FieldType::ReadOnly;
            }
            fields.push(f);
        }
    }
}
fn set_path(root: &mut toml::Value, path: &[String], value: Option<toml::Value>) -> Result<()> {
    let (key, rest) = path.split_first().context("empty field path")?;
    if let toml::Value::Array(array) = root {
        let index: usize = key.parse()?;
        if rest.is_empty() {
            if let Some(value) = value {
                array[index] = value;
            }
        } else {
            set_path(&mut array[index], rest, value)?;
        }
    } else {
        let table = root.as_table_mut().context("invalid field parent")?;
        if rest.is_empty() {
            if let Some(value) = value {
                table.insert(key.clone(), value);
            } else {
                table.remove(key);
            }
        } else {
            set_path(
                table.get_mut(key).context("missing field parent")?,
                rest,
                value,
            )?;
        }
    }
    Ok(())
}
fn node_mut<'a>(mut root: &'a mut toml::Value, path: &[String]) -> Result<&'a mut toml::Value> {
    for key in path {
        root = if let toml::Value::Array(array) = root {
            array
                .get_mut(key.parse::<usize>()?)
                .context("missing list item")?
        } else {
            root.get_mut(key).context("missing field")?
        };
    }
    Ok(root)
}
fn item_template(path: &[String]) -> Result<toml::Value> {
    let key = path.last().map(String::as_str).unwrap_or("");
    let text = match key {
        "zones" => "id = 0\nname = \"\"",
        "default_credentials" => "username = \"\"\nsource = \"https://\"\nnote = \"\"",
        "probe" | "manual" | "set" | "automatic" => {
            "netfn = 0\ncommand = 0\ndata = []\ndelay_ms = 0"
        }
        "examples" => "percent = 0\nzone = 0\nexpected = []",
        "expected" => return Ok(toml::Value::Array(vec![])),
        "manufacturer_ids" | "product_ids" | "percent_values" | "raw_values" | "expect" => {
            return Ok(toml::Value::Integer(0));
        }
        _ if path.iter().any(|p| p == "expected") => return Ok(toml::Value::Integer(0)),
        _ => return Ok(toml::Value::String(String::new())),
    };
    Ok(toml::Value::Table(toml::from_str(text)?))
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
    fn all_adapters_roundtrip_without_losing_nested_data() -> Result<()> {
        let registry = bmcforge_profiles::Registry::load(None)?;
        for p in registry.profiles.values() {
            let before = toml::Value::try_from(p)?;
            let mut f = Form::new(
                p.id.clone(),
                EditKind::Profile(p.id.clone()),
                &toml::to_string_pretty(p)?,
            )?;
            let after = bmcforge_profiles::Profile::parse(&f.text()?)?;
            assert_eq!(before, toml::Value::try_from(after)?, "{}", p.id);
            for field in &f.fields {
                if field.path.last().is_some_and(|p| p == "password") {
                    assert!(matches!(field.kind, FieldType::Secret));
                }
            }
        }
        Ok(())
    }
    #[test]
    fn adapter_lists_can_add_edit_reorder_remove_and_enable_optional_control() -> Result<()> {
        let p = bmcforge_profiles::Registry::load(None)?.profiles["generic"].clone();
        let mut f = Form::new(
            p.id.clone(),
            EditKind::Profile(p.id.clone()),
            &toml::to_string_pretty(&p)?,
        )?;
        f.focus = f.fields.iter().position(|f| f.path == ["sources"]).unwrap();
        f.input(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE));
        assert!(f.error.is_empty(), "{}", f.error);
        let index = f
            .fields
            .iter()
            .position(|f| f.path == ["sources", "1"])
            .unwrap();
        f.focus = index;
        f.fields[index].input = Editor::new("https://example.com/new");
        f.input(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
        let value: toml::Value = toml::from_str(&f.text()?)?;
        assert_eq!(
            value["sources"][0].as_str(),
            Some("https://example.com/new")
        );
        f.focus = f
            .fields
            .iter()
            .position(|f| f.path == ["sources", "0"])
            .unwrap();
        f.input(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(
            toml::from_str::<toml::Value>(&f.text()?)?["sources"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        f.focus = f.fields.iter().position(|f| f.path == ["fan"]).unwrap();
        f.input(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE));
        assert!(f.error.is_empty(), "{}", f.error);
        assert!(f.fields.iter().any(|f| f.path == ["fan", "zones"]));
        f.focus = f.fields.iter().position(|f| f.path == ["fan"]).unwrap();
        f.input(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        assert!(
            toml::from_str::<toml::Value>(&f.text()?)?
                .get("fan")
                .is_none()
        );
        Ok(())
    }
    #[test]
    fn settings_group_reset_only_changes_its_group_and_all_reset_covers_every_field() -> Result<()>
    {
        let mut settings = Settings {
            language: "zh-CN".into(),
            theme: "light".into(),
            concurrency: 17,
            scan_port: 624,
            sensor_filter: "fan".into(),
            ..Settings::default()
        };
        settings.keys.insert("edit".into(), "z".into());
        let mut f = Form::new(
            "Settings".into(),
            EditKind::Settings,
            &toml::to_string_pretty(&settings)?,
        )?;
        f.focus = f.fields.iter().position(|f| f.path == ["theme"]).unwrap();
        f.reset_settings(false)?;
        let reset: Settings = toml::from_str(&f.text()?)?;
        assert_eq!(reset.language, "en");
        assert_eq!(reset.theme, "dark");
        assert_eq!(reset.scan_port, 624);
        assert_eq!(reset.concurrency, 17);
        assert_eq!(reset.keys["edit"], "z");
        f.reset_settings(true)?;
        assert_eq!(
            toml::from_str::<toml::Value>(&f.text()?)?,
            toml::Value::try_from(Settings::default())?
        );
        Ok(())
    }
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

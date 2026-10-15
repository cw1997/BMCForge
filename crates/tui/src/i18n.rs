use anyhow::{Context, Result, ensure};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema_version: u32,
    id: String,
    name: String,
    #[serde(default)]
    override_builtin: bool,
}
pub struct Pack {
    pub name: String,
    bundle: FluentBundle<FluentResource>,
    pub plain: BTreeMap<String, String>,
    variables: BTreeMap<String, BTreeSet<String>>,
}
pub struct Locales {
    pub packs: BTreeMap<String, Pack>,
    pub diagnostics: Vec<String>,
    english: Pack,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Message {
    pub id: String,
    pub args: BTreeMap<String, String>,
    pub details: String,
}
impl Message {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            args: BTreeMap::new(),
            details: String::new(),
        }
    }
    pub fn detail(id: &str, details: impl Into<String>) -> Self {
        Self {
            details: details.into(),
            ..Self::new(id)
        }
    }
}
impl Default for Message {
    fn default() -> Self {
        Self::new("connecting")
    }
}
impl From<String> for Message {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}
impl From<&str> for Message {
    fn from(s: &str) -> Self {
        match s {
            "Ready" => Self::new("ready"),
            "Online" => Self::new("online"),
            "Connecting" => Self::new("connecting"),
            _ => Self::detail("technical-event", s),
        }
    }
}
impl Pack {
    fn parse(metadata: &str, text: &str) -> Result<(Metadata, Self)> {
        let meta: Metadata = toml::from_str(metadata)?;
        ensure!(
            meta.schema_version == 1 && !meta.name.is_empty(),
            "invalid locale metadata"
        );
        let lang = meta.id.parse().context("invalid locale tag")?;
        let resource = FluentResource::try_new(text.to_owned())
            .map_err(|(_, e)| anyhow::anyhow!("Fluent syntax: {e:?}"))?;
        let mut bundle = FluentBundle::new(vec![lang]);
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .map_err(|e| anyhow::anyhow!("duplicate Fluent messages: {e:?}"))?;
        let ast = fluent_syntax::parser::parse(text)
            .map_err(|(_, e)| anyhow::anyhow!("Fluent: {e:?}"))?;
        let mut variables: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        fn vars(pattern: &fluent_syntax::ast::Pattern<&str>, out: &mut BTreeSet<String>) {
            use fluent_syntax::ast::*;
            fn inline(e: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
                match e {
                    InlineExpression::VariableReference { id } => {
                        out.insert(id.name.into());
                    }
                    InlineExpression::Placeable { expression } => expr(expression, out),
                    InlineExpression::FunctionReference { arguments, .. } => {
                        arguments_vars(arguments, out)
                    }
                    InlineExpression::TermReference {
                        arguments: Some(arguments),
                        ..
                    } => arguments_vars(arguments, out),
                    _ => {}
                }
            }
            fn arguments_vars(
                a: &fluent_syntax::ast::CallArguments<&str>,
                out: &mut BTreeSet<String>,
            ) {
                for arg in &a.positional {
                    inline(arg, out);
                }
                for arg in &a.named {
                    inline(&arg.value, out);
                }
            }
            fn expr(e: &Expression<&str>, out: &mut BTreeSet<String>) {
                match e {
                    Expression::Inline(e) => inline(e, out),
                    Expression::Select { selector, variants } => {
                        inline(selector, out);
                        for v in variants {
                            vars(&v.value, out);
                        }
                    }
                }
            }
            for e in &pattern.elements {
                if let PatternElement::Placeable { expression } = e {
                    expr(expression, out);
                }
            }
        }
        let mut plain = BTreeMap::new();
        for entry in ast.body {
            if let fluent_syntax::ast::Entry::Message(msg) = entry {
                let p = msg.value.context("messages require values")?;
                let mut v = BTreeSet::new();
                vars(&p, &mut v);
                let pattern = bundle
                    .get_message(msg.id.name)
                    .and_then(|m| m.value())
                    .context("missing pattern")?;
                if v.is_empty() {
                    let mut errors = vec![];
                    let value = bundle
                        .format_pattern(pattern, None, &mut errors)
                        .into_owned();
                    ensure!(
                        errors.is_empty(),
                        "invalid message {}: {errors:?}",
                        msg.id.name
                    );
                    plain.insert(msg.id.name.into(), value);
                }
                variables.insert(msg.id.name.into(), v);
            }
        }
        // Exercise variable patterns as well as plain messages before accepting a pack.
        for (id, vars) in &variables {
            let mut args = FluentArgs::new();
            for v in vars {
                args.set(v, 2);
            }
            let mut errors = vec![];
            if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
                let _ = bundle.format_pattern(pattern, Some(&args), &mut errors);
            }
            ensure!(
                errors.is_empty(),
                "invalid Fluent references in {id}: {errors:?}"
            );
        }
        Ok((
            meta,
            Self {
                name: String::new(),
                bundle,
                plain,
                variables,
            },
        ))
    }
}
impl Locales {
    pub fn load(dir: Option<&Path>) -> Result<Self> {
        let (_, english) = Pack::parse(
            include_str!("../../../locales/en/locale.toml"),
            include_str!("../../../locales/en/main.ftl"),
        )?;
        let mut locales = Self {
            packs: BTreeMap::new(),
            diagnostics: vec![],
            english,
        };
        macro_rules! builtin {
            ($id:literal) => {{
                let (m, mut p) = Pack::parse(
                    include_str!(concat!("../../../locales/", $id, "/locale.toml")),
                    include_str!(concat!("../../../locales/", $id, "/main.ftl")),
                )?;
                p.name = m.name;
                locales.packs.insert(m.id, p);
            }};
        }
        builtin!("en");
        builtin!("zh-CN");
        builtin!("zh-TW");
        builtin!("ja");
        builtin!("ko");
        builtin!("de");
        builtin!("fr");
        builtin!("es");
        builtin!("pt-BR");
        builtin!("it");
        builtin!("ru");
        let english = &locales.packs["en"].variables;
        for (id, p) in &locales.packs {
            ensure!(
                &p.variables == english,
                "builtin locale {id}: keys/variables differ from English"
            );
        }
        if let Some(dir) = dir {
            let mut entries = match std::fs::read_dir(dir).and_then(|e| {
                e.map(|e| e.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()
            }) {
                Ok(e) => e,
                Err(e) => {
                    locales.diagnostics.push(format!("{}: {e}", dir.display()));
                    return Ok(locales);
                }
            };
            entries.sort();
            let mut seen = BTreeSet::new();
            for path in entries.into_iter().filter(|p| p.is_dir()) {
                let result = (|| -> Result<()> {
                    let (m, mut p) = Pack::parse(
                        &std::fs::read_to_string(path.join("locale.toml"))?,
                        &std::fs::read_to_string(path.join("main.ftl"))?,
                    )?;
                    ensure!(
                        seen.insert(m.id.clone()),
                        "duplicate external locale {}",
                        m.id
                    );
                    ensure!(
                        !locales.packs.contains_key(&m.id) || m.override_builtin,
                        "{} requires override_builtin = true",
                        m.id
                    );
                    for (key, vars) in &p.variables {
                        ensure!(
                            locales.english.variables.get(key) == Some(vars),
                            "unknown message or variable mismatch: {key}"
                        );
                    }
                    p.name = m.name;
                    locales.packs.insert(m.id, p);
                    Ok(())
                })();
                if let Err(e) = result {
                    locales
                        .diagnostics
                        .push(format!("{}: {e:#}", path.display()));
                }
            }
        }
        Ok(locales)
    }
    pub fn text<'a>(&'a self, language: &str, id: &'a str) -> &'a str {
        self.packs
            .get(language)
            .and_then(|p| p.plain.get(id))
            .or_else(|| self.english.plain.get(id))
            .map(String::as_str)
            .unwrap_or(id)
    }
    pub fn render(&self, language: &str, message: &Message) -> String {
        let pack = self
            .packs
            .get(language)
            .filter(|p| p.variables.contains_key(&message.id))
            .unwrap_or(&self.english);
        let mut args = FluentArgs::new();
        for (k, v) in &message.args {
            if let Ok(n) = v.parse::<i64>() {
                args.set(k, n);
            } else {
                args.set(k, v.as_str());
            }
        }
        let mut errors = vec![];
        let text = pack
            .bundle
            .get_message(&message.id)
            .and_then(|m| m.value())
            .map(|p| {
                pack.bundle
                    .format_pattern(p, Some(&args), &mut errors)
                    .into_owned()
            })
            .unwrap_or_else(|| message.id.clone());
        if message.details.is_empty() {
            text
        } else {
            format!("{text}: {}", message.details)
        }
    }
    pub fn schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Metadata)).expect("locale schema")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_complete_plural_and_fallback() -> Result<()> {
        let l = Locales::load(None)?;
        assert_eq!(l.packs.len(), 11);
        let mut m = Message::new("device-count");
        m.args.insert("count".into(), "2".into());
        assert!(l.render("en", &m).contains("2 devices"));
        assert_eq!(l.text("missing", "fleet"), "Fleet");
        assert_eq!(l.text("ja", "fleet"), "サーバー");
        Ok(())
    }
    #[test]
    fn broken_external_does_not_prevent_startup() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("bad");
        std::fs::create_dir(&p)?;
        std::fs::write(p.join("locale.toml"), "invalid")?;
        let l = Locales::load(Some(dir.path()))?;
        assert_eq!(l.diagnostics.len(), 1);
        assert_eq!(l.packs.len(), 11);
        Ok(())
    }
    #[test]
    fn partial_packs_fallback_and_overrides_require_explicit_intent() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let write = |folder: &str, metadata: &str, ftl: &str| -> Result<()> {
            let p = dir.path().join(folder);
            std::fs::create_dir_all(&p)?;
            std::fs::write(p.join("locale.toml"), metadata)?;
            std::fs::write(p.join("main.ftl"), ftl)?;
            Ok(())
        };
        write(
            "nl",
            "schema_version = 1\nid = 'nl'\nname = 'Nederlands'",
            "ready = Gereed",
        )?;
        write(
            "override",
            "schema_version = 1\nid = 'en'\nname = 'Custom English'",
            "ready = Custom",
        )?;
        write(
            "mismatch",
            "schema_version = 1\nid = 'sv'\nname = 'Svenska'",
            "device-count = { $wrong }",
        )?;
        write(
            "syntax",
            "schema_version = 1\nid = 'da'\nname = 'Dansk'",
            "ready = { broken",
        )?;
        let l = Locales::load(Some(dir.path()))?;
        assert_eq!(l.diagnostics.len(), 3);
        assert_eq!(l.text("nl", "ready"), "Gereed");
        assert_eq!(l.text("nl", "fleet"), "Fleet");
        assert_eq!(l.text("en", "ready"), "Ready");
        write(
            "override",
            "schema_version = 1\nid = 'en'\nname = 'Custom English'\noverride_builtin = true",
            "ready = Custom",
        )?;
        let l = Locales::load(Some(dir.path()))?;
        assert_eq!(l.text("en", "ready"), "Custom");
        assert_eq!(l.text("en", "fleet"), "Fleet");
        assert_eq!(l.text("nl", "fleet"), "Fleet");
        let mut m = Message::new("device-count");
        for (count, ending) in [(1, "устройство"), (2, "устройства"), (5, "устройств")]
        {
            m.args.insert("count".into(), count.to_string());
            assert!(l.render("ru", &m).ends_with(ending));
        }
        Ok(())
    }
}

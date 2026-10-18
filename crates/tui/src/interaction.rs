use crate::{
    app::{App, Modal},
    mouse::Target,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
};
use std::time::{Duration, Instant};

pub const ACTIONS: &[&str] = &[
    "quit",
    "search",
    "import",
    "discover",
    "settings",
    "profiles",
    "logs",
    "export",
    "edit",
    "manual",
    "automatic",
    "select",
];

#[derive(Default)]
pub struct Interaction {
    pub light: bool,
    pub leader: Option<Instant>,
    pub hover: Option<Target>,
    pub pressed: Option<(Target, Instant)>,
    pub viewport: Option<(Rect, usize, Option<std::mem::Discriminant<Modal>>)>,
    pub action_focus: Option<usize>,
    pub action_offset: usize,
    pub actions: Vec<Target>,
    pub menu_items: Vec<(String, Target)>,
}
impl Interaction {
    pub fn press(&mut self, target: Target) {
        self.pressed = Some((target, Instant::now()));
    }
    pub fn style(&self, target: &Target) -> Style {
        let theme = crate::theme::Theme::named(if self.light { "light" } else { "dark" });
        if self
            .pressed
            .as_ref()
            .is_some_and(|(t, time)| t == target && time.elapsed() < Duration::from_millis(150))
        {
            Style::default()
                .fg(theme.background)
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD)
        } else if self.hover.as_ref() == Some(target) {
            Style::default().fg(theme.background).bg(theme.accent)
        } else {
            Style::default()
        }
    }
}

pub fn key_label(key: KeyCode) -> String {
    match key {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        _ => format!("{key:?}"),
    }
}
impl App {
    pub fn target_key(&self, target: &Target) -> Option<KeyCode> {
        match target {
            Target::Page(n) => char::from_digit(*n as u32 + 1, 10).map(KeyCode::Char),
            Target::Key(k) => Some(*k),
            Target::Language => Some(KeyCode::F(3)),
            Target::Shortcut(name) => self
                .config
                .settings
                .keys
                .get(name)
                .and_then(|s| s.chars().next())
                .map(KeyCode::Char),
            _ => None,
        }
    }
    pub fn target_label(&self, target: &Target) -> String {
        if matches!(target, Target::Shortcut(name) if name.starts_with("theme:")) {
            return "Enter".into();
        }
        if matches!(target, Target::Palette) {
            return "Ctrl+P".into();
        }
        self.target_key(target)
            .map(key_label)
            .unwrap_or_else(|| "F8".into())
    }
    pub fn keyboard_feedback(&mut self, key: KeyEvent) {
        self.interaction.hover = None;
        if let Some(target) = self
            .hits
            .iter()
            .map(|h| &h.target)
            .find(|t| self.target_key(t) == Some(key.code))
            .cloned()
        {
            self.interaction.press(target);
        } else if let Some(Modal::Form(form)) = &self.modal {
            self.interaction.press(Target::Field(form.focus));
        } else {
            let target = match &self.modal {
                Some(Modal::FieldChoice { cursor, .. }) => Some(Target::ChoiceRow(*cursor)),
                Some(Modal::Languages { cursor }) => Some(Target::LanguageRow(*cursor)),
                Some(Modal::ModelChoice { query, cursor }) => self
                    .model_options(query)
                    .get(*cursor)
                    .cloned()
                    .map(Target::ModelRow),
                None if self.page == 0 => {
                    self.current().map(|s| Target::Server(s.id.clone(), false))
                }
                None if self.page == 2 => self
                    .registry
                    .profiles
                    .keys()
                    .nth(self.profile_cursor)
                    .cloned()
                    .map(Target::Profile),
                _ => None,
            };
            if let Some(t) = target {
                self.interaction.press(t);
            }
        }
    }
}

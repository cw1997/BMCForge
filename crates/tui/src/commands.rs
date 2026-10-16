use crate::{
    app::{Action, App, Modal},
    mouse::Target,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};
pub struct Command {
    pub label: String,
    pub target: Target,
    pub reason: String,
}
impl App {
    pub fn target_reason(&self, target: &Target) -> String {
        match target {
            Target::Shortcut(name) => {
                if matches!(name.as_str(), "edit" | "manual" | "automatic" | "select")
                    && self.current().is_none()
                    && !(name == "select" && self.page == 2)
                {
                    self.text("command-needs-server").into()
                } else {
                    self.command_reason(name)
                }
            }
            Target::Key(KeyCode::Delete) if self.current().is_none() => {
                self.text("command-needs-server").into()
            }
            _ => String::new(),
        }
    }
    pub fn commands(&self, query: &str) -> Vec<Command> {
        let mut entries = Vec::new();
        for (page, name) in ["fleet", "sensors", "discovery", "profiles", "activity"]
            .iter()
            .enumerate()
        {
            entries.push(Command {
                label: self.text(name).into(),
                target: Target::Page(page),
                reason: String::new(),
            });
        }
        for name in crate::interaction::ACTIONS {
            let unavailable = matches!(*name, "edit" | "manual" | "automatic" | "select")
                && self.current().is_none()
                && !(*name == "select" && self.page == 2);
            entries.push(Command {
                label: self
                    .text(if *name == "import" && self.page == 2 {
                        "identify"
                    } else {
                        name
                    })
                    .into(),
                target: Target::Shortcut((*name).into()),
                reason: if unavailable {
                    self.text("command-needs-server").into()
                } else {
                    self.command_reason(name)
                },
            });
        }
        for (name, key) in [
            ("language", KeyCode::F(3)),
            ("mouse", KeyCode::F(4)),
            ("help", KeyCode::F(1)),
            ("remove", KeyCode::Delete),
        ] {
            entries.push(Command {
                label: self.text(name).into(),
                target: Target::Key(key),
                reason: if key == KeyCode::Delete && self.current().is_none() {
                    self.text("command-needs-server").into()
                } else {
                    String::new()
                },
            });
        }
        let mut contextual = Vec::new();
        if self.page == 2 {
            contextual.extend([
                ("select-all", KeyCode::Char('A')),
                ("filter", KeyCode::Char('f')),
            ]);
            contextual.push(("/24", KeyCode::Char('G')));
        }
        if self.identifying {
            contextual.push(("cancel", KeyCode::Char('c')));
        }
        if self.page == 3 {
            contextual.extend([
                ("confirm", KeyCode::Enter),
                ("credentials", KeyCode::Char('u')),
            ]);
        }
        for (name, code) in contextual {
            entries.push(Command {
                label: if name == "/24" {
                    name.into()
                } else {
                    self.text(name).into()
                },
                target: Target::Key(code),
                reason: String::new(),
            });
        }
        for theme in ["dark", "light"] {
            entries.push(Command {
                label: format!("{}: {}", self.text("command-theme"), self.text(theme)),
                target: Target::Shortcut(format!("theme:{theme}")),
                reason: String::new(),
            });
        }
        let query = query.to_lowercase();
        entries
            .into_iter()
            .filter(|c| {
                format!("{} {}", c.label, self.target_label(&c.target))
                    .to_lowercase()
                    .contains(&query)
            })
            .collect()
    }
    pub fn open_commands(&mut self, query: String) {
        self.interaction.leader = None;
        self.interaction.action_focus = None;
        self.modal = Some(Modal::Commands { query, cursor: 0 });
    }
    pub fn command_input(&mut self, key: KeyEvent) -> Option<Vec<Action>> {
        if let Some(Modal::Commands { query, cursor }) = &self.modal {
            let mut query = query.clone();
            let mut cursor = *cursor;
            let entries = self.commands(&query);
            match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    self.modal = Some(Modal::Commands { query, cursor });
                }
                KeyCode::Down => {
                    cursor = (cursor + 1).min(entries.len().saturating_sub(1));
                    self.modal = Some(Modal::Commands { query, cursor });
                }
                KeyCode::Enter => {
                    if let Some(command) = entries.get(cursor)
                        && command.reason.is_empty()
                    {
                        self.modal = None;
                        if let Target::Shortcut(name) = &command.target
                            && let Some(theme) = name.strip_prefix("theme:")
                        {
                            let previous = self.config.settings.theme.clone();
                            self.config.settings.theme = theme.into();
                            if !self.demo
                                && let Err(error) = self.config.save(&self.path)
                            {
                                self.config.settings.theme = previous;
                                self.modal = Some(Modal::Info(error.to_string()));
                            }
                            return Some(vec![]);
                        }
                        return Some(self.invoke_target(command.target.clone()));
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                    self.modal = Some(Modal::Commands { query, cursor: 0 });
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    query.push(c);
                    self.modal = Some(Modal::Commands { query, cursor: 0 });
                }
                _ => {}
            }
            return Some(vec![]);
        }
        if self.modal.is_some() {
            self.interaction.leader = None;
            return None;
        }
        if let Some(start) = self.interaction.leader.take()
            && start.elapsed() < Duration::from_secs(2)
        {
            match key.code {
                KeyCode::Char('p') => self.open_commands(String::new()),
                KeyCode::Char('t') => self.open_commands(self.text("command-theme").into()),
                KeyCode::Char('1'..='5') => {
                    if let KeyCode::Char(c) = key.code {
                        self.page = c as usize - '1' as usize;
                    }
                }
                KeyCode::Char('h') => {
                    return Some(self.invoke_target(Target::Key(KeyCode::F(1))));
                }
                KeyCode::Char('q') => return Some(vec![Action::Quit]),
                _ => {}
            }
            return Some(vec![]);
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('p') => {
                    self.open_commands(String::new());
                    return Some(vec![]);
                }
                KeyCode::Char('x') => {
                    self.interaction.leader = Some(Instant::now());
                    return Some(vec![]);
                }
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmcforge_core::config::Config;
    use ratatui::{Terminal, backend::TestBackend};
    fn app() -> App {
        App::new(
            Config::default(),
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        )
    }
    fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Action> {
        app.input(KeyEvent::new(code, modifiers))
    }
    #[test]
    fn palette_filters_disables_and_preserves_text_input() {
        let mut app = app();
        key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(matches!(app.modal, Some(Modal::Commands { .. })));
        app.paste("manual");
        assert!(app.commands("manual").iter().all(|c| !c.reason.is_empty()));
        assert!(key(&mut app, KeyCode::Enter, KeyModifiers::NONE).is_empty());
        assert!(matches!(app.modal, Some(Modal::Commands { .. })));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.open_commands(String::new());
        app.paste("no-such-command-123");
        assert!(app.commands("no-such-command-123").is_empty());
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.modal = Some(Modal::Info("owned".into()));
        key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(matches!(app.modal, Some(Modal::Info(_))));
    }
    #[test]
    fn leader_navigation_timeout_cancel_and_theme() {
        let mut app = app();
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('4'), KeyModifiers::NONE);
        assert_eq!(app.page, 3);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.interaction.leader.is_none());
        app.interaction.leader = Some(Instant::now() - Duration::from_secs(3));
        key(&mut app, KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(app.page, 0);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(app.commands(app.text("command-theme")).len(), 2);
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.settings.theme, "light");
        assert!(
            key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE)
                .iter()
                .any(|a| matches!(a, Action::Quit))
        );
    }
    #[test]
    fn pages_and_details_render_for_empty_and_unicode_fleets() {
        for theme in ["dark", "light"] {
            for (width, height) in [(12, 5), (30, 12), (79, 24), (80, 24), (120, 40)] {
                for populated in [false, true] {
                    let mut app = app();
                    app.config.settings.theme = theme.into();
                    app.config.settings.language = "zh-CN".into();
                    if populated {
                        app.config.servers = bmcforge_ipmi::mock::servers();
                        app.config.servers[0].name =
                            "服务器测试 Unicode 窄窗口 long-name".repeat(4);
                    }
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    for page in 0..5 {
                        app.page = page;
                        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                        assert!(
                            app.hits
                                .iter()
                                .all(|h| h.area.right() <= width && h.area.bottom() <= height)
                        );
                    }
                    if populated && (20..80).contains(&width) && height >= 8 {
                        app.page = 0;
                        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
                        assert!(matches!(app.modal, Some(Modal::Info(_))));
                        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn palette_renders_scrolls_and_mouse_activates_in_both_themes() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        for theme in ["dark", "light"] {
            for (width, height) in [(20, 8), (30, 12), (79, 24), (80, 24), (120, 40), (180, 50)] {
                let mut app = app();
                app.config.settings.theme = theme.into();
                app.open_commands(String::new());
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                for _ in 0..35 {
                    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                }
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                assert!(
                    app.hits
                        .iter()
                        .all(|h| h.area.right() <= width && h.area.bottom() <= height)
                );
                app.open_commands(app.text("profiles").into());
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                if let Some(hit) = app
                    .hits
                    .iter()
                    .find(|h| matches!(h.target, Target::CommandRow(0)))
                    .cloned()
                {
                    app.mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: hit.area.x,
                        row: hit.area.y,
                        modifiers: KeyModifiers::NONE,
                    });
                    assert_eq!(app.page, 3);
                    assert!(app.modal.is_none());
                }
            }
        }
    }
}

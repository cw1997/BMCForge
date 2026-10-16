use crate::{
    app::App,
    mouse::{Hit, Pane, Target},
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn panel(theme: Theme) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(theme.surface())
}

pub fn truncate(text: &str, width: u16) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if text.width() <= usize::from(width) {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let size = c.width().unwrap_or(0);
        if used + size > usize::from(width - 1) {
            break;
        }
        out.push(c);
        used += size;
    }
    out.push('…');
    out
}

pub fn popup(frame: &mut Frame, area: Rect, theme: Theme) {
    let viewport = frame.area();
    for cell in frame.buffer_mut().content.iter_mut() {
        let dim = |color: Color, fallback: Color| match color {
            Color::Rgb(r, g, b) => match theme.background {
                Color::Rgb(br, bg, bb) => Color::Rgb(
                    ((u16::from(r) + u16::from(br)) / 2) as u8,
                    ((u16::from(g) + u16::from(bg)) / 2) as u8,
                    ((u16::from(b) + u16::from(bb)) / 2) as u8,
                ),
                _ => fallback,
            },
            _ => fallback,
        };
        cell.fg = dim(cell.fg, theme.muted);
        cell.bg = dim(cell.bg, theme.background);
        cell.modifier.remove(Modifier::BOLD);
    }
    let shadow = Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width,
        area.height,
    )
    .intersection(viewport);
    frame.render_widget(Clear, shadow);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.shadow)),
        shadow,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme.surface()), area);
}

/// The scrollbar and its click targets share exactly the same clipped geometry.
/// `maximum` is the last reachable cursor/scroll position, not a content length.
pub fn scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    maximum: usize,
    pane: Pane,
    theme: Theme,
) -> Vec<Hit> {
    let area = area.intersection(frame.area());
    if maximum == 0 || area.height < 2 || area.width == 0 {
        return vec![];
    }
    let mut hits = Vec::new();
    let travel = usize::from(area.height - 1);
    let thumb = position.min(maximum).saturating_mul(travel) / maximum;
    for row in 0..area.height {
        let offset = usize::from(row);
        let symbol = if offset == thumb {
            "█"
        } else if row == 0 {
            "↑"
        } else if row == area.height - 1 {
            "↓"
        } else {
            "│"
        };
        let rect = Rect::new(area.right() - 1, area.y + row, 1, 1);
        frame.render_widget(
            Paragraph::new(symbol).style(
                Style::default()
                    .fg(if offset == thumb {
                        theme.accent
                    } else {
                        theme.muted
                    })
                    .bg(theme.surface),
            ),
            rect,
        );
        hits.push(Hit::new(
            rect,
            Target::ScrollTo {
                pane,
                position: offset.saturating_mul(maximum) / travel,
            },
        ));
    }
    hits
}

pub fn text_height(text: &str, width: u16) -> usize {
    Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .line_count(width)
}

impl App {
    pub fn status_text(&self) -> String {
        let text = if self.identifying {
            format!(
                "{} {}/{}",
                self.text("identifying"),
                self.pending.len(),
                self.identify_total
            )
        } else if self.scanning {
            format!(
                "{} {}/{}",
                self.text("scanning"),
                self.discovery.len(),
                self.scan_total
            )
        } else {
            self.locales
                .render(&self.config.settings.language, &self.status)
        };
        bmcforge_core::redact(&text, &self.config.secrets())
    }
}

impl App {
    pub fn menu_input(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<Vec<crate::app::Action>> {
        use crate::app::Modal;
        use crossterm::event::KeyCode;
        if !matches!(self.modal, Some(Modal::Actions { .. })) {
            return None;
        }
        let Some(Modal::Actions {
            previous,
            items,
            mut cursor,
        }) = self.modal.take()
        else {
            unreachable!()
        };
        match key.code {
            KeyCode::Esc => {
                self.modal = previous.map(|m| *m);
            }
            KeyCode::Enter => {
                if let Some((_, target)) = items.get(cursor) {
                    if !self.target_reason(target).is_empty() {
                        self.modal = Some(Modal::Actions {
                            previous,
                            items,
                            cursor,
                        });
                        return Some(vec![]);
                    }
                    self.modal = previous.map(|m| *m);
                    return Some(self.invoke_target(target.clone()));
                }
                self.modal = previous.map(|m| *m);
            }
            _ => {
                match key.code {
                    KeyCode::Up | KeyCode::BackTab => cursor = cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab => {
                        cursor = (cursor + 1).min(items.len().saturating_sub(1))
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Actions {
                    previous,
                    items,
                    cursor,
                });
            }
        }
        Some(vec![])
    }
    pub fn scroll_to(&mut self, pane: Pane, position: usize) {
        use crate::app::Modal;
        match pane {
            Pane::Fleet => self.cursor = position.min(self.visible().len().saturating_sub(1)),
            Pane::Discovery => {
                self.scan_cursor = position.min(self.scan_visible().len().saturating_sub(1))
            }
            Pane::Profiles => {
                self.profile_cursor = position.min(self.registry.profiles.len().saturating_sub(1));
                self.model_scroll = 0;
            }
            Pane::Detail => self.detail_scroll = position.min(u16::MAX as usize) as u16,
            Pane::Models => {
                self.model_scroll = position.min(self.catalog.models.len().saturating_sub(1))
            }
            Pane::Sensors => self.sensor_scroll = position,
            Pane::Logs => self.log_scroll = position.min(u16::MAX as usize) as u16,
            Pane::Modal => match &mut self.modal {
                Some(
                    Modal::Actions { cursor, .. }
                    | Modal::Commands { cursor, .. }
                    | Modal::Languages { cursor }
                    | Modal::FieldChoice { cursor, .. }
                    | Modal::ModelChoice { cursor, .. },
                ) => *cursor = position,
                Some(Modal::Form(form)) => {
                    form.focus = position.min(form.fields.len().saturating_sub(1))
                }
                Some(Modal::Editor { editor, .. }) => {
                    editor.row = position.min(editor.lines.len().saturating_sub(1));
                    editor.scroll = editor.row;
                    editor.col = editor.col.min(editor.lines[editor.row].chars().count());
                }
                _ => self.modal_scroll = position.min(u16::MAX as usize) as u16,
            },
        }
    }
}

/// Render wrapped text and return the clamped offset with its scrollbar targets.
pub fn paragraph(
    frame: &mut Frame,
    area: Rect,
    text: &str,
    title: &str,
    position: u16,
    pane: Pane,
    theme: Theme,
) -> (u16, Vec<Hit>) {
    let inner = panel(theme).inner(area);
    let maximum = text_height(text, inner.width)
        .saturating_sub(usize::from(inner.height))
        .min(u16::MAX as usize);
    let position = usize::from(position).min(maximum);
    frame.render_widget(
        Paragraph::new(text)
            .block(panel(theme).title(title.to_owned()))
            .wrap(Wrap { trim: false })
            .scroll((position as u16, 0)),
        area,
    );
    let track = Rect::new(
        area.right().saturating_sub(1),
        inner.y,
        1.min(area.width),
        inner.height,
    );
    (
        position as u16,
        scrollbar(frame, track, position, maximum, pane, theme),
    )
}

#[derive(Clone, Copy)]
pub enum Tone {
    Ready,
    Working,
    Warning,
    Error,
    Neutral,
}
pub fn tone(id: &str) -> Tone {
    if id.contains("failed") || id == "offline" {
        Tone::Error
    } else if id.contains("uncertain") || id.contains("unknown") {
        Tone::Warning
    } else if matches!(id, "online" | "automatic" | "ready") {
        Tone::Ready
    } else if id.contains("connecting") || id.contains("identif") || id.contains("scanning") {
        Tone::Working
    } else {
        Tone::Neutral
    }
}
pub fn badge(theme: Theme, label: String, tone: Tone) -> ratatui::widgets::Cell<'static> {
    let (marker, color) = match tone {
        Tone::Ready => ("●", theme.success),
        Tone::Working => ("…", theme.accent),
        Tone::Warning => ("?", theme.warning),
        Tone::Error => ("!", theme.danger),
        Tone::Neutral => ("·", theme.muted),
    };
    ratatui::widgets::Cell::from(format!("[{marker} {label}]")).style(
        Style::default()
            .fg(color)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{Action, Modal},
        editor::Editor,
    };
    use bmcforge_core::config::Config;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{Terminal, backend::TestBackend};
    fn app() -> App {
        App::new(
            Config {
                servers: bmcforge_ipmi::mock::servers(),
                ..Default::default()
            },
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        )
    }
    fn click(app: &mut App, hit: &Hit) -> Vec<Action> {
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.area.x,
            row: hit.area.y,
            modifiers: KeyModifiers::NONE,
        })
    }
    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    fn key(app: &mut App, key: KeyCode) {
        app.input(KeyEvent::new(key, KeyModifiers::NONE));
    }

    #[test]
    fn truncation_respects_wide_characters_controls_and_zero_width() {
        assert_eq!(truncate("中文AB", 4), "中…");
        assert_eq!(truncate("中文AB", 0), "");
        assert_eq!(truncate("中文AB", 1), "…");
        assert_eq!(truncate("A\nB\tC", 10), "A B C");
        for width in 0..12 {
            assert!(truncate("服务器 A中 é long text", width).width() <= usize::from(width));
        }
    }
    #[test]
    fn reversed_status_is_full_width_and_opens_only_redacted_text() {
        for name in ["dark", "light"] {
            let mut app = app();
            app.config.settings.theme = name.into();
            app.config.servers[0].credentials.password = "secret-needle".into();
            app.status = crate::i18n::Message::detail(
                "operation-failed",
                format!("服务器 secret-needle {}", "very long message ".repeat(100)),
            );
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|f| {
                    crate::ui::draw(f, &mut app);
                    let row = app
                        .hits
                        .iter()
                        .find(|h| h.target == Target::Status)
                        .unwrap()
                        .area
                        .y;
                    for x in 0..80 {
                        assert_eq!(f.buffer_mut()[(x, row)].bg, Theme::named(name).foreground);
                    }
                })
                .unwrap();
            let hit = app
                .hits
                .iter()
                .find(|h| h.target == Target::Status)
                .unwrap()
                .clone();
            let theme = Theme::named(name);
            let mut skip = 0;
            for x in 0..80 {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                skip = terminal.backend().buffer()[(x, hit.area.y)]
                    .symbol()
                    .width()
                    .saturating_sub(1);
                assert_eq!(
                    terminal.backend().buffer()[(x, hit.area.y)].bg,
                    theme.foreground
                );
            }
            assert_eq!(
                terminal.backend().buffer()[(79, hit.area.y)].fg,
                theme.background
            );
            assert!(!text(&terminal).contains("secret-needle"));
            assert!(text(&terminal).contains('…'));
            app.mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: hit.area.x,
                row: hit.area.y,
                modifiers: KeyModifiers::NONE,
            });
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            assert_eq!(
                terminal.backend().buffer()[(79, hit.area.y)].bg,
                theme.foreground
            );
            click(&mut app, &hit);
            assert!(
                matches!(&app.modal,Some(Modal::Info(message)) if message.contains("[REDACTED]") && !message.contains("secret-needle"))
            );
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            assert!(app.hits.iter().any(|h| matches!(
                h.target,
                Target::ScrollTo {
                    pane: Pane::Modal,
                    ..
                }
            )));
        }
    }
    #[test]
    fn short_status_paints_trailing_blanks_and_progress_does_not_replace_navigation() {
        let mut app = app();
        app.status = "Ready".into();
        app.scanning = true;
        app.scan_total = 100;
        app.interaction.leader = Some(std::time::Instant::now());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let hit = app
            .hits
            .iter()
            .find(|h| h.target == Target::Status)
            .unwrap();
        let cell = &terminal.backend().buffer()[(79, hit.area.y)];
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.bg, Theme::named("dark").foreground);
        assert!(text(&terminal).contains("/100"));
        assert!(text(&terminal).contains("Ctrl+X →"));
    }
    #[test]
    fn shadows_are_clipped_do_not_own_input_and_disappear_on_close() {
        for theme in ["dark", "light"] {
            for (width, height) in [(20, 8), (30, 12), (80, 24), (120, 40)] {
                let mut app = app();
                app.config.settings.theme = theme.into();
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                let before = terminal.backend().buffer().clone();
                app.modal = Some(Modal::Info("Popup text".into()));
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                let popup = Rect::new(
                    width / 20,
                    height / 12,
                    width - width / 10,
                    height - height / 6,
                );
                let right = popup.right();
                let row = popup.y + 1;
                if right < width && row < height {
                    assert_eq!(
                        terminal.backend().buffer()[(right, row)].bg,
                        Theme::named(theme).shadow
                    );
                    assert!(
                        app.hits
                            .iter()
                            .all(|h| !h.area.contains((right, row).into()))
                    );
                }
                assert!(
                    app.hits
                        .iter()
                        .all(|h| h.area.right() <= width && h.area.bottom() <= height)
                );
                app.modal = None;
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                assert_eq!(&before, terminal.backend().buffer());
            }
        }
    }
    #[test]
    fn mouse_preferences_persist_and_action_menu_restores_draft() {
        let mut app = app();
        assert!(app.config.settings.mouse);
        let mut terminal = Terminal::new(TestBackend::new(30, 16)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let palette = app
            .hits
            .iter()
            .find(|h| h.target == Target::Palette)
            .unwrap()
            .clone();
        app.config.settings.mouse = false;
        click(&mut app, &palette);
        assert!(app.modal.is_none());
        key(&mut app, KeyCode::F(4));
        assert!(!app.config.settings.mouse);
        app.config.settings.mouse = true;
        click(&mut app, &palette);
        assert!(matches!(app.modal, Some(Modal::Commands { .. })));
        key(&mut app, KeyCode::Esc);
        app.invoke_target(Target::Shortcut("settings".into()));
        if let Some(Modal::Form(form)) = app.modal.take() {
            app.advanced_form = Some(form);
        }
        let mut terminal = Terminal::new(TestBackend::new(20, 16)).unwrap();
        app.modal = Some(Modal::Editor {
            title: "Draft".into(),
            kind: crate::app::EditKind::Search,
            editor: Editor::new("保留草稿"),
            error: String::new(),
        });
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let more = app
            .hits
            .iter()
            .find(|h| h.target == Target::Overflow)
            .unwrap()
            .clone();
        click(&mut app, &more);
        assert!(matches!(app.modal, Some(Modal::Actions { .. })));
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        assert!(app.hits.iter().all(|h| !matches!(
            h.target,
            Target::Palette | Target::Status | Target::Server(..)
        )));
        key(&mut app, KeyCode::Esc);
        assert!(matches!(&app.modal,Some(Modal::Editor{editor,..}) if editor.text()=="保留草稿"));
    }
    #[test]
    fn clicked_scrollbars_move_to_end_and_follow_the_pointer() {
        let mut app = app();
        app.page = 2;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let last = app
            .hits
            .iter()
            .rfind(|h| {
                matches!(
                    h.target,
                    Target::ScrollTo {
                        pane: Pane::Profiles,
                        ..
                    }
                )
            })
            .unwrap()
            .clone();
        click(&mut app, &last);
        assert_eq!(app.profile_cursor, app.registry.profiles.len() - 1);
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        app.modal = Some(Modal::Info(
            (0..100).map(|n| format!("Line {n}\n")).collect(),
        ));
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let last = app
            .hits
            .iter()
            .rfind(|h| {
                matches!(
                    h.target,
                    Target::ScrollTo {
                        pane: Pane::Modal,
                        ..
                    }
                )
            })
            .unwrap()
            .clone();
        click(&mut app, &last);
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        assert!(text(&terminal).contains("Line 99"));
        let position = app.modal_scroll;
        app.mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: last.area.x,
            row: last.area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.modal_scroll, position - 1);
        let mut smaller = Terminal::new(TestBackend::new(30, 12)).unwrap();
        smaller.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        assert!(
            app.hits
                .iter()
                .all(|h| h.area.right() <= 30 && h.area.bottom() <= 12)
        );
    }
    #[test]
    fn disabled_actions_are_muted_and_menu_activation_has_no_side_effects() {
        let mut app = app();
        app.config.servers.clear();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let manual = Target::Shortcut("manual".into());
        assert!(!app.target_reason(&manual).is_empty());
        let hit = app
            .hits
            .iter()
            .find(|h| h.target == manual)
            .unwrap()
            .clone();
        assert_eq!(
            terminal.backend().buffer()[(hit.area.x, hit.area.y)].fg,
            Theme::named("dark").muted
        );
        let more = app
            .hits
            .iter()
            .find(|h| h.target == Target::Overflow)
            .unwrap()
            .clone();
        click(&mut app, &more);
        if let Some(Modal::Actions { items, cursor, .. }) = &mut app.modal {
            *cursor = items
                .iter()
                .position(|(_, target)| *target == manual)
                .unwrap();
        } else {
            panic!("action menu was not opened");
        }
        assert!(
            app.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .is_empty()
        );
        assert!(matches!(app.modal, Some(Modal::Actions { .. })));
        key(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        let mut updated = app.config.clone();
        updated.settings.mouse = false;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        updated.save(&path).unwrap();
        assert!(!Config::load(&path).unwrap().settings.mouse);
    }

    #[test]
    fn every_dialog_uses_shadow_and_preserves_cancel_safety() {
        for theme in ["dark", "light"] {
            let mut app = app();
            app.config.settings.theme = theme.into();
            let dialogs = vec![
                Modal::Commands {
                    query: String::new(),
                    cursor: 0,
                },
                Modal::Languages { cursor: 0 },
                Modal::Preview {
                    hosts: vec!["192.0.2.1".into(); 50],
                    discovery: false,
                    duplicates: 0,
                    errors: vec![],
                },
                Modal::Confirm {
                    title: "Confirm".into(),
                    text: "Server\n".repeat(80),
                    control: vec![],
                    manual: None,
                },
                Modal::Import {
                    servers: vec![],
                    authenticate: false,
                    duplicates: 0,
                },
                Modal::Remove {
                    id: "demo".into(),
                    name: "服务器".into(),
                },
                Modal::ModelChoice {
                    query: String::new(),
                    cursor: 0,
                },
            ];
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            for dialog in dialogs {
                app.modal = Some(dialog);
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                assert_eq!(
                    terminal.backend().buffer()[(76, 3)].bg,
                    Theme::named(theme).shadow
                );
                assert!(
                    app.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                        .is_empty()
                );
                assert!(app.modal.is_none());
            }
        }
    }
}

use crate::{
    app::{Action, App, Modal},
    editor::Editor,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug)]
pub enum Target {
    Page(usize),
    Key(KeyCode),
    Shortcut(String),
    Language,
    Server(String, bool),
    Discovery(String, bool),
    Profile(String),
    LanguageRow(usize),
    ModelRow(String),
    Editor { top: usize, left: usize },
    Scroll(Pane),
}
#[derive(Clone, Copy, Debug)]
pub enum Pane {
    Fleet,
    Discovery,
    Profiles,
    Detail,
    Sensors,
    Logs,
    Modal,
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub area: Rect,
    pub target: Target,
}
impl Hit {
    pub fn new(area: Rect, target: Target) -> Self {
        Self { area, target }
    }
}
pub fn place(editor: &mut Editor, row: usize, column: usize) {
    editor.row = row.min(editor.lines.len().saturating_sub(1));
    let mut width = 0;
    editor.col = 0;
    for c in editor.lines[editor.row].chars() {
        let size = c.width().unwrap_or(0);
        if width + size > column {
            break;
        }
        width += size;
        editor.col += 1;
    }
}
impl App {
    pub fn mouse(&mut self, event: MouseEvent) -> Vec<Action> {
        if !self.config.settings.mouse {
            return vec![];
        }
        let Some(hit) = self
            .hits
            .iter()
            .rev()
            .find(|h| h.area.contains((event.column, event.row).into()))
            .cloned()
        else {
            return vec![];
        };
        if self.modal.is_some()
            && !matches!(
                hit.target,
                Target::Key(_)
                    | Target::LanguageRow(_)
                    | Target::ModelRow(_)
                    | Target::Editor { .. }
                    | Target::Scroll(Pane::Modal)
            )
        {
            return vec![];
        }
        if matches!(
            event.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        ) {
            let down = event.kind == MouseEventKind::ScrollDown;
            let pane = match &hit.target {
                Target::Scroll(p) => *p,
                Target::Server(..) => Pane::Fleet,
                Target::Discovery(..) => Pane::Discovery,
                Target::Profile(_) => Pane::Profiles,
                Target::Editor { .. } | Target::LanguageRow(_) | Target::ModelRow(_) => Pane::Modal,
                _ => return vec![],
            };
            let step = |v: usize| {
                if down {
                    v.saturating_add(1)
                } else {
                    v.saturating_sub(1)
                }
            };
            match pane {
                Pane::Fleet => {
                    self.cursor = step(self.cursor).min(self.visible().len().saturating_sub(1))
                }
                Pane::Discovery => {
                    self.scan_cursor =
                        step(self.scan_cursor).min(self.scan_visible().len().saturating_sub(1))
                }
                Pane::Profiles => {
                    self.profile_cursor = step(self.profile_cursor)
                        .min(self.registry.profiles.len().saturating_sub(1))
                }
                Pane::Detail => {
                    self.detail_scroll =
                        step(self.detail_scroll.into()).min(u16::MAX as usize) as u16
                }
                Pane::Sensors => {
                    self.sensor_scroll = step(self.sensor_scroll).min(
                        self.current()
                            .and_then(|s| self.snapshots.get(&s.id))
                            .map_or(0, |s| s.sensors.len().saturating_sub(1)),
                    )
                }
                Pane::Logs => {
                    self.log_scroll = step(self.log_scroll.into()).min(u16::MAX as usize) as u16
                }
                Pane::Modal => match &mut self.modal {
                    Some(Modal::Languages { .. } | Modal::ModelChoice { .. }) => {
                        return self.input(KeyEvent::new(
                            if down { KeyCode::Down } else { KeyCode::Up },
                            KeyModifiers::NONE,
                        ));
                    }
                    Some(Modal::Editor { editor, .. }) => {
                        editor.scroll =
                            step(editor.scroll).min(editor.lines.len().saturating_sub(1));
                        editor.row = step(editor.row).min(editor.lines.len().saturating_sub(1));
                        editor.col = editor.col.min(editor.lines[editor.row].chars().count());
                    }
                    _ => {
                        self.modal_scroll =
                            step(self.modal_scroll.into()).min(u16::MAX as usize) as u16
                    }
                },
            }
            return vec![];
        }
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return vec![];
        }
        let key = match hit.target {
            Target::Page(page) => {
                self.page = page;
                return vec![];
            }
            Target::Language => {
                self.modal = Some(Modal::Languages { cursor: 0 });
                return vec![];
            }
            Target::Key(key) if self.modal.is_none() => {
                return match self.fixed_input(KeyEvent::new(key, KeyModifiers::NONE)) {
                    Ok(actions) => actions,
                    Err(e) => {
                        self.log(format!("{e:#}"));
                        vec![]
                    }
                };
            }
            Target::Key(key) => key,
            Target::Shortcut(name) => {
                let Some(c) = self
                    .config
                    .settings
                    .keys
                    .get(&name)
                    .and_then(|s| s.chars().next())
                else {
                    return vec![];
                };
                KeyCode::Char(c)
            }
            Target::Server(id, checkbox) => {
                if let Some(index) = self
                    .visible()
                    .iter()
                    .position(|i| self.config.servers[*i].id == id)
                {
                    self.cursor = index;
                }
                if checkbox {
                    if !self.selected.remove(&id) {
                        self.selected.insert(id);
                    }
                    return vec![];
                }
                let double = self.last_click.as_ref().is_some_and(|(last, time)| {
                    last == &id && time.elapsed() <= Duration::from_millis(400)
                });
                self.last_click = Some((id, Instant::now()));
                if double {
                    KeyCode::Enter
                } else {
                    return vec![];
                }
            }
            Target::Discovery(host, checkbox) => {
                if let Some(index) = self
                    .scan_visible()
                    .iter()
                    .position(|i| self.discovery[*i].host == host)
                {
                    self.scan_cursor = index;
                }
                if checkbox && !self.discovery_selected.remove(&host) {
                    self.discovery_selected.insert(host);
                }
                return vec![];
            }
            Target::Profile(id) => {
                if let Some(index) = self.registry.profiles.keys().position(|p| p == &id) {
                    self.profile_cursor = index;
                }
                return vec![];
            }
            Target::LanguageRow(index) => {
                if let Some(Modal::Languages { cursor }) = &mut self.modal {
                    *cursor = index;
                }
                KeyCode::Enter
            }
            Target::ModelRow(id) => {
                if let Some(Modal::ModelChoice { query, .. }) = &self.modal {
                    let index = self.model_options(query).iter().position(|m| m == &id);
                    if let (Some(index), Some(Modal::ModelChoice { cursor, .. })) =
                        (index, &mut self.modal)
                    {
                        *cursor = index;
                    }
                }
                KeyCode::Enter
            }
            Target::Editor { top, left } => {
                if let Some(Modal::Editor { editor, .. }) = &mut self.modal {
                    place(
                        editor,
                        top + usize::from(event.row - hit.area.y),
                        left + usize::from(event.column - hit.area.x),
                    );
                }
                return vec![];
            }
            Target::Scroll(_) => return vec![],
        };
        self.input(KeyEvent::new(key, KeyModifiers::NONE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_cursor_hit_does_not_split_characters() {
        let mut e = Editor::new("A中日B\n한국어");
        place(&mut e, 0, 2);
        assert_eq!(e.col, 1);
        place(&mut e, 0, 3);
        assert_eq!(e.col, 2);
        place(&mut e, 1, 4);
        assert_eq!(e.col, 2);
    }
}

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug)]
pub struct Editor {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    pub scroll: usize,
}
impl Editor {
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(String::from).collect(),
            row: 0,
            col: 0,
            scroll: 0,
        }
    }
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
    fn byte_index(&self) -> usize {
        self.lines[self.row]
            .char_indices()
            .nth(self.col)
            .map(|(i, _)| i)
            .unwrap_or(self.lines[self.row].len())
    }
    pub fn input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let i = self.byte_index();
                self.lines[self.row].insert(i, c);
                self.col += 1;
            }
            KeyCode::Enter => {
                let i = self.byte_index();
                let tail = self.lines[self.row].split_off(i);
                self.row += 1;
                self.lines.insert(self.row, tail);
                self.col = 0;
            }
            KeyCode::Backspace if self.col > 0 => {
                self.col -= 1;
                let i = self.byte_index();
                self.lines[self.row].remove(i);
            }
            KeyCode::Backspace if self.row > 0 => {
                let tail = self.lines.remove(self.row);
                self.row -= 1;
                self.col = self.lines[self.row].chars().count();
                self.lines[self.row].push_str(&tail);
            }
            KeyCode::Delete => {
                let i = self.byte_index();
                if i < self.lines[self.row].len() {
                    self.lines[self.row].remove(i);
                } else if self.row + 1 < self.lines.len() {
                    let tail = self.lines.remove(self.row + 1);
                    self.lines[self.row].push_str(&tail);
                }
            }
            KeyCode::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].chars().count();
                }
            }
            KeyCode::Right => {
                if self.col < self.lines[self.row].chars().count() {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            KeyCode::Up => {
                self.row = self.row.saturating_sub(1);
                self.clamp();
            }
            KeyCode::Down => {
                self.row = (self.row + 1).min(self.lines.len() - 1);
                self.clamp();
            }
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.lines[self.row].chars().count(),
            _ => {}
        }
    }
    fn clamp(&mut self) {
        self.col = self.col.min(self.lines[self.row].chars().count());
    }
    pub fn paste(&mut self, text: &str) {
        for c in text.replace("\r\n", "\n").replace('\r', "\n").chars() {
            self.input(KeyEvent::new(
                if c == '\n' {
                    KeyCode::Enter
                } else {
                    KeyCode::Char(c)
                },
                KeyModifiers::NONE,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_editing_and_paste() {
        let mut e = Editor::new("");
        e.paste("服务器\r\n密码");
        assert_eq!(e.text(), "服务器\n密码");
        e.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "服务器\n密");
        e.input(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        e.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "服务器密");
    }
}

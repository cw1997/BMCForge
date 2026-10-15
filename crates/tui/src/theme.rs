use ratatui::style::{Color, Style};
#[derive(Clone, Copy)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub selection: Color,
    pub danger: Color,
    pub warning: Color,
}
impl Theme {
    pub fn named(name: &str) -> Self {
        if name == "light" {
            Self {
                background: Color::Rgb(250, 250, 250),
                surface: Color::Rgb(239, 239, 239),
                foreground: Color::Rgb(35, 35, 35),
                muted: Color::Rgb(100, 100, 100),
                accent: Color::Rgb(118, 65, 160),
                selection: Color::Rgb(224, 212, 238),
                danger: Color::Rgb(175, 35, 45),
                warning: Color::Rgb(138, 85, 0),
            }
        } else {
            Self {
                background: Color::Rgb(20, 20, 20),
                surface: Color::Rgb(30, 30, 30),
                foreground: Color::Rgb(225, 225, 225),
                muted: Color::Rgb(150, 150, 150),
                accent: Color::Rgb(180, 145, 220),
                selection: Color::Rgb(61, 47, 75),
                danger: Color::Rgb(245, 125, 135),
                warning: Color::Rgb(235, 190, 110),
            }
        }
    }
    pub fn base(self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }
    pub fn selected(self) -> Style {
        Style::default().fg(self.foreground).bg(self.selection)
    }
}

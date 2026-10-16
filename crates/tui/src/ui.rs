use crate::app::{App, Modal};
use crate::mouse::{Hit, Pane, Target};
use bmcforge_core::ControlState;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState, Wrap},
};
use unicode_width::UnicodeWidthStr;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let viewport = (
        frame.area(),
        app.page,
        app.modal.as_ref().map(std::mem::discriminant),
    );
    if app.interaction.viewport != Some(viewport) {
        app.interaction.hover = None;
        app.interaction.action_focus = None;
        app.interaction.action_offset = 0;
        app.interaction.viewport = Some(viewport);
    }
    if !app.config.settings.mouse {
        app.interaction.hover = None;
    }
    app.hits.clear();
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    app.interaction.light = app.config.settings.theme == "light";
    let bg = theme.background;
    let fg = theme.foreground;
    if frame.area().width < 20 || frame.area().height < 8 {
        frame.render_widget(
            Paragraph::new(format!(
                "BMCForge\n{}\nCtrl+C / q",
                app.text("terminal-too-small")
            ))
            .style(theme.base()),
            frame.area(),
        );
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(bg).fg(fg)),
        frame.area(),
    );
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(frame.area());
    let title = format!(
        " BMCForge {} {} ",
        env!("CARGO_PKG_VERSION"),
        if app.demo { "[DEMO]" } else { "" }
    );
    let nav = Block::default()
        .borders(Borders::BOTTOM)
        .title(title)
        .style(Style::default().bg(theme.surface));
    let inner = nav.inner(layout[0]);
    frame.render_widget(nav, layout[0]);
    let language_width = 18.min(inner.width / 3);
    let left = Rect {
        width: inner.width.saturating_sub(language_width),
        ..inner
    };
    let lang = Rect {
        x: left.right(),
        width: language_width,
        ..inner
    };
    let mut x = left.x;
    for (page, id) in ["fleet", "sensors", "discovery", "profiles", "activity"]
        .iter()
        .enumerate()
    {
        let label = if left.width < 60 {
            format!("{}", page + 1)
        } else {
            format!("{} {}", page + 1, app.text(id))
        };
        let label = if page == app.page {
            format!("▸{label}")
        } else {
            format!(" {label}")
        };
        let width = (label.width() as u16 + 1).min(left.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let r = Rect::new(x, left.y, width, 1);
        frame.render_widget(
            Paragraph::new(label).style(
                (if page == app.page {
                    theme.focus()
                } else {
                    Style::default()
                })
                .patch(app.interaction.style(&Target::Page(page))),
            ),
            r,
        );
        app.hits.push(Hit::new(r, Target::Page(page)));
        x = x.saturating_add(width);
    }
    frame.render_widget(
        Paragraph::new(format!(
            "[F3 {} ▾]",
            app.locales
                .packs
                .get(&app.config.settings.language)
                .map(|p| p.name.as_str())
                .unwrap_or("English")
        ))
        .style(app.interaction.style(&Target::Language)),
        lang,
    );
    app.hits.push(Hit::new(lang, Target::Language));
    match app.page {
        0 => fleet(frame, layout[1], app),
        1 => sensors(frame, layout[1], app),
        2 => discovery(frame, layout[1], app),
        3 => profiles(frame, layout[1], app),
        _ => logs(frame, layout[1], app),
    }
    let empty = match app.page {
        0 => app.visible().is_empty(),
        1 => app.current().is_none(),
        2 => app.discovery.is_empty() && !app.scanning,
        3 => app.registry.profiles.is_empty(),
        _ => app.logs.is_empty(),
    };
    if empty && layout[1].height > 3 {
        frame.render_widget(
            Paragraph::new(format!("{}  · Ctrl+P", app.text("command-empty")))
                .style(Style::default().fg(theme.muted).bg(theme.background)),
            Rect::new(
                layout[1].x + 1,
                layout[1].y + 2,
                layout[1].width.saturating_sub(2),
                1,
            ),
        );
    }
    let actions = [
        "import",
        "discover",
        "edit",
        "manual",
        "automatic",
        "select",
        "search",
        "settings",
        "export",
        "quit",
    ];
    let footer_lines = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(layout[2]);
    let mut buttons = actions
        .iter()
        .map(|name| (app.text(name).to_owned(), Target::Shortcut((*name).into())))
        .collect::<Vec<_>>();
    buttons.push((app.text("language").into(), Target::Language));
    buttons.push((app.text("mouse").into(), Target::Key(KeyCode::F(4))));
    buttons.push(("?".into(), Target::Key(KeyCode::F(1))));
    buttons.push((app.text("remove").into(), Target::Key(KeyCode::Delete)));
    if app.page == 2 {
        buttons.push((
            app.text("select-all").into(),
            Target::Key(KeyCode::Char('A')),
        ));
        buttons.push(("/24".into(), Target::Key(KeyCode::Char('G'))));
        buttons.push((app.text("filter").into(), Target::Key(KeyCode::Char('f'))));
        buttons.push((
            app.text("identify").into(),
            Target::Shortcut("import".into()),
        ));
    }
    if app.page == 3 {
        buttons.push((app.text("confirm").into(), Target::Key(KeyCode::Enter)));
        buttons.push((
            app.text("credentials").into(),
            Target::Key(KeyCode::Char('u')),
        ));
    }
    if app.identifying {
        buttons.push((app.text("cancel").into(), Target::Key(KeyCode::Char('c'))));
    }
    if app.page == 2 {
        buttons.rotate_right(if app.identifying { 5 } else { 4 });
    }
    if app.page == 3 {
        buttons.rotate_right(if app.identifying { 3 } else { 2 });
    }
    if app.page == 0 {
        buttons.rotate_left(2);
    }
    if app.page == 1 {
        buttons.rotate_left(3);
    }
    let palette_width = 12.min(footer_lines[0].width);
    let palette_area = Rect::new(
        footer_lines[0].x,
        footer_lines[0].y,
        palette_width,
        footer_lines[0].height,
    );
    frame.render_widget(
        Paragraph::new(" Ctrl+P ▾ ").style(theme.focus()),
        palette_area,
    );
    app.hits.push(Hit::new(palette_area, Target::Palette));
    let action_area = Rect::new(
        palette_area.right(),
        footer_lines[0].y,
        footer_lines[0].width.saturating_sub(palette_width),
        footer_lines[0].height,
    );
    button_bar(frame, action_area, app, buttons);
    let status = app.status_text();
    let status_tone = crate::chrome::tone(&app.status.id);
    let marker = if app.identifying || app.scanning {
        format!(" {} ", spinner())
    } else {
        match status_tone {
            crate::chrome::Tone::Error => " [!] ".into(),
            crate::chrome::Tone::Warning => " [?] ".into(),
            _ => " ".into(),
        }
    };
    let inverse_theme = crate::theme::Theme::named(if app.config.settings.theme == "light" {
        "dark"
    } else {
        "light"
    });
    let marker_color = match status_tone {
        crate::chrome::Tone::Error => inverse_theme.danger,
        crate::chrome::Tone::Warning => inverse_theme.warning,
        _ => theme.background,
    };
    let available = footer_lines[1].width.saturating_sub(marker.width() as u16);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                marker,
                Style::default()
                    .fg(marker_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(crate::chrome::truncate(&status, available)),
        ]))
        .style(theme.status()),
        footer_lines[1],
    );
    // Wide-character continuation cells can be reset by the text renderer.
    // Paint the whole strip after text so even those cells keep reverse video.
    for x in footer_lines[1].x..footer_lines[1].right() {
        let cell = &mut frame.buffer_mut()[(x, footer_lines[1].y)];
        cell.set_bg(theme.foreground);
        if cell.symbol() == " " {
            cell.set_fg(theme.background);
        }
    }
    app.hits.push(Hit::new(footer_lines[1], Target::Status));
    let navigation = if app
        .interaction
        .leader
        .is_some_and(|time| time.elapsed() < std::time::Duration::from_secs(2))
    {
        "Ctrl+X → p · 1–5 · t · h · q · Esc".into()
    } else {
        format!(
            "Ctrl+P · F8: {} · ↑↓ j/k · Enter · Tab: 1–5 · F4: {} {}",
            app.text("form-actions"),
            app.text("mouse"),
            if app.config.settings.mouse {
                "●"
            } else {
                "○"
            }
        )
    };
    frame.render_widget(
        Paragraph::new(crate::chrome::truncate(&navigation, footer_lines[2].width))
            .style(Style::default().fg(theme.muted)),
        footer_lines[2],
    );
    if app.modal.is_some() {
        app.hits.clear(); // A modal owns all input, including clicks outside its content.
        modal(frame, app);
    }
    // Apply feedback after rendering so tables and nested checkbox targets share the same treatment.
    for hit in &app.hits {
        if !matches!(
            hit.target,
            Target::Scroll(_) | Target::ScrollTo { .. } | Target::Editor { .. }
        ) {
            let style = if app.target_reason(&hit.target).is_empty() {
                app.interaction.style(&hit.target)
            } else {
                Style::default()
            };
            if style != Style::default() {
                frame.buffer_mut().set_style(hit.area, style);
            }
        }
    }
}
fn spinner() -> char {
    use std::{sync::OnceLock, time::Instant};
    static START: OnceLock<Instant> = OnceLock::new();
    ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
        [(START.get_or_init(Instant::now).elapsed().as_millis() / 100 % 10) as usize]
}
fn header(theme: crate::theme::Theme, cells: &[&str]) -> Row<'static> {
    Row::new(cells.iter().map(|s| Cell::from(s.to_string()))).style(
        Style::default()
            .fg(theme.accent)
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD),
    )
}
fn fleet(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let parts = if area.width >= 120 {
        Layout::horizontal([Constraint::Percentage(64), Constraint::Percentage(36)]).split(area)
    } else {
        Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(if area.width < 80 {
                0
            } else if area.height >= 18 {
                7
            } else {
                4
            }),
        ])
        .split(area)
    };
    let visible = app.visible();
    let rows: Vec<_> = visible
        .iter()
        .map(|i| {
            let s = &app.config.servers[*i];
            let snapshot = app.snapshots.get(&s.id);
            let state = snapshot
                .map(|s| app.locales.render(&app.config.settings.language, &s.status))
                .unwrap_or_else(|| app.text("connecting").into());
            let rpm = snapshot
                .and_then(|s| {
                    s.sensors
                        .iter()
                        .filter(|v| v.unit == "RPM")
                        .filter_map(|v| v.value)
                        .reduce(f64::max)
                })
                .map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| "—".into());
            let cells = vec![
                Cell::from(if app.selected.contains(&s.id) {
                    "[x]"
                } else {
                    "[ ]"
                }),
                Cell::from(s.tags.join(",")),
                Cell::from(s.name.clone()),
                Cell::from(s.endpoint()),
                crate::chrome::badge(
                    theme,
                    state,
                    snapshot.map_or(crate::chrome::Tone::Working, |s| {
                        crate::chrome::tone(&s.status.id)
                    }),
                ),
                Cell::from(rpm),
            ];
            Row::new(
                cells
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| area.width >= 80 || matches!(index, 0 | 2 | 4))
                    .map(|(_, cell)| cell),
            )
        })
        .collect();
    let widths = if area.width < 80 {
        vec![
            Constraint::Length(3),
            Constraint::Percentage(45),
            Constraint::Min(8),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Percentage(15),
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Min(12),
            Constraint::Length(8),
        ]
    };
    let headings = if area.width < 80 {
        vec!["", app.text("server"), app.text("status")]
    } else {
        vec![
            "",
            app.text("group"),
            app.text("server"),
            app.text("bmc-endpoint"),
            app.text("status"),
            app.text("max-rpm"),
        ]
    };
    let table = Table::new(rows, widths)
        .header(header(theme, &headings))
        .block(crate::chrome::panel(theme).title(format!(
            " {} — {}: {} — {}: {} ",
            app.text("fleet"),
            app.text("selected"),
            app.selected.len(),
            app.text("filter"),
            app.search
        )))
        .row_highlight_style(Style::default().bg(theme.selection).fg(theme.foreground));
    let mut state = TableState::default().with_selected(if visible.is_empty() {
        None
    } else {
        Some(app.cursor.min(visible.len() - 1))
    });
    frame.render_stateful_widget(table, parts[0], &mut state);
    table_hits(
        frame,
        app,
        parts[0],
        state.offset(),
        visible
            .iter()
            .map(|i| Target::Server(app.config.servers[*i].id.clone(), false))
            .collect(),
        Pane::Fleet,
    );
    app.hits
        .push(Hit::new(parts[1], Target::Scroll(Pane::Detail)));
    let (position, hits) = crate::chrome::paragraph(
        frame,
        parts[1],
        &server_detail(app),
        app.text("server-details"),
        app.detail_scroll,
        Pane::Detail,
        theme,
    );
    app.detail_scroll = position;
    app.hits.extend(hits);
}
pub(crate) fn server_detail(app: &App) -> String {
    let Some(server) = app.current() else {
        return app
            .text("press-i-to-import-servers-or-d-to-discover-a-subnet-each-server")
            .into();
    };
    let snapshot = app.snapshots.get(&server.id);
    let identity = snapshot
        .map(|s| {
            format!(
                "{} | IANA {} | ID 0x{:04x} | {}: {}",
                s.identity.model,
                s.identity.manufacturer_id,
                s.identity.product_id,
                app.text("firmware"),
                s.identity.firmware
            )
        })
        .unwrap_or_else(|| app.text("identifying").into());
    let control = snapshot
        .map(|s| match s.control {
            ControlState::Unknown => app.text("unknown").into(),
            ControlState::Automatic => format!("[{}]", app.text("automatic")),
            ControlState::Requested { percent, zone } => {
                format!("{}: {percent}% / {zone}", app.text("manual"))
            }
            ControlState::RawRequested { value, zone } => {
                format!("{}: {value:#04x} / {zone}", app.text("raw"))
            }
            ControlState::Uncertain => format!("[! {}]", app.text("uncertain")),
        })
        .unwrap_or_else(|| app.text("unknown").into());
    let candidates = snapshot
        .map(|s| app.registry.candidates(&s.identity).join(", "))
        .unwrap_or_default();
    format!(
        "{identity}\n{}: {} | {}: {} | ID: {}\n{}: {control} | {}: {}\n{}\n{}",
        app.text("profile"),
        server.profile.as_deref().unwrap_or("generic"),
        app.text("candidates"),
        if candidates.is_empty() {
            "—"
        } else {
            &candidates
        },
        server.model_id.as_deref().unwrap_or("—"),
        app.text("control"),
        app.text("actual-duty"),
        app.text("unknown"),
        snapshot
            .map(|s| app.capability(server, &s.identity, server.profile.as_deref()))
            .unwrap_or_default(),
        app.text("safety")
    )
}
fn sensors(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let parts = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(5),
    ])
    .split(area);
    app.hits
        .push(Hit::new(parts[0], Target::Scroll(Pane::Detail)));
    app.hits
        .push(Hit::new(parts[1], Target::Scroll(Pane::Sensors)));
    let (position, hits) = crate::chrome::paragraph(
        frame,
        parts[0],
        &server_detail(app),
        app.text("fan-control-identity"),
        app.detail_scroll,
        Pane::Detail,
        theme,
    );
    app.detail_scroll = position;
    app.hits.extend(hits);
    let snapshot = app.current().and_then(|s| app.snapshots.get(&s.id));
    let filter = app.config.settings.sensor_filter.to_lowercase();
    let count = snapshot
        .into_iter()
        .flat_map(|s| &s.sensors)
        .filter(|s| {
            format!("{} {}", s.name, s.kind)
                .to_lowercase()
                .contains(&filter)
        })
        .count();
    let maximum = count.saturating_sub(usize::from(parts[1].height.saturating_sub(3)));
    app.sensor_scroll = app.sensor_scroll.min(maximum);
    let rows: Vec<_> = snapshot
        .into_iter()
        .flat_map(|s| &s.sensors)
        .filter(|s| {
            format!("{} {}", s.name, s.kind)
                .to_lowercase()
                .contains(&filter)
        })
        .skip(app.sensor_scroll)
        .map(|s| {
            let fahrenheit = app.config.settings.temperature_unit == "F" && s.unit == "°C";
            let unit = if fahrenheit { "°F" } else { &s.unit };
            let value = s
                .value
                .map(|v| format!("{:.2}", if fahrenheit { v * 1.8 + 32.0 } else { v }))
                .unwrap_or_else(|| "—".into());
            let cells = vec![
                s.id.clone(),
                s.name.clone(),
                s.kind.clone(),
                value,
                unit.into(),
                s.state.clone(),
                s.raw.iter().map(|b| format!("{b:02x} ")).collect(),
            ];
            Row::new(
                cells
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| area.width >= 80 || matches!(index, 1 | 3 | 4 | 5))
                    .map(|(_, cell)| cell),
            )
        })
        .collect();
    let widths = if area.width < 80 {
        vec![
            Constraint::Percentage(38),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Min(8),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Min(12),
            Constraint::Length(14),
        ]
    };
    let headings = if area.width < 80 {
        vec![
            app.text("sensor"),
            app.text("reading"),
            app.text("unit"),
            app.text("state"),
        ]
    } else {
        vec![
            "ID",
            app.text("sensor"),
            app.text("type"),
            app.text("reading"),
            app.text("unit"),
            app.text("state"),
            app.text("raw"),
        ]
    };
    frame.render_widget(
        Table::new(rows, widths)
            .header(header(theme, &headings))
            .block(
                crate::chrome::panel(theme)
                    .title(app.text("sensors-up-down-switch-server-pgup-pgdn-scroll-details")),
            ),
        parts[1],
    );
    app.hits.extend(crate::chrome::scrollbar(
        frame,
        Rect::new(
            parts[1].right().saturating_sub(1),
            parts[1].y + 2,
            1.min(parts[1].width),
            parts[1].height.saturating_sub(3),
        ),
        app.sensor_scroll,
        maximum,
        Pane::Sensors,
        theme,
    ));
    let data: Vec<u64> = app
        .current()
        .and_then(|s| app.history.get(&s.id))
        .map(|v| v.iter().copied().collect())
        .unwrap_or_default();
    frame.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::default().fg(theme.accent))
            .block(crate::chrome::panel(theme).title(app.text("max-rpm"))),
        parts[2],
    );
}
fn discovery(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let visible = app.scan_visible();
    let rows: Vec<_> = visible
        .iter()
        .map(|i| {
            let r = &app.discovery[*i];
            Row::new(vec![
                if app.discovery_selected.contains(&r.host) {
                    "[x]"
                } else {
                    "[ ]"
                }
                .into(),
                r.host.clone(),
                app.text(match r.status {
                    bmcforge_ipmi::discovery::ProbeStatus::IpmiCandidate => "unauthenticated",
                    _ => "unknown",
                })
                .into(),
                if r.status == bmcforge_ipmi::discovery::ProbeStatus::IpmiCandidate {
                    app.text("unauthenticated").into()
                } else {
                    app.text("scan-finished").into()
                },
            ])
        })
        .collect();
    let mut state = TableState::default().with_selected(if visible.is_empty() {
        None
    } else {
        Some(app.scan_cursor.min(visible.len() - 1))
    });
    frame.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Min(10),
            ],
        )
        .header(header(
            crate::theme::Theme::named(&app.config.settings.theme),
            &["", "BMC IPv4", app.text("probe-status"), app.text("note")],
        ))
        .block(crate::chrome::panel(theme).title(format!(
            " {}/{} {} | {}: {} | i: {} ",
            app.discovery.len(),
            app.scan_total,
            app.text(if app.scanning { "scanning" } else { "idle" }),
            app.text("selected"),
            app.discovery_selected.len(),
            app.text("identify")
        )))
        .row_highlight_style(Style::default().bg(theme.selection).fg(theme.foreground)),
        area,
        &mut state,
    );
    table_hits(
        frame,
        app,
        area,
        state.offset(),
        visible
            .iter()
            .map(|i| Target::Discovery(app.discovery[*i].host.clone(), false))
            .collect(),
        Pane::Discovery,
    );
}
fn verification<'a>(app: &'a App, v: &bmcforge_profiles::Verification) -> &'a str {
    use bmcforge_profiles::Verification::*;
    app.text(match v {
        HardwareVerified => "hardware-verified",
        Documented => "documented",
        Experimental => "experimental",
        Unsupported => "unsupported",
    })
}
fn profiles(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let parts = if area.width >= 120 {
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area)
    };
    let rows: Vec<_> = app
        .registry
        .profiles
        .values()
        .map(|p| {
            Row::new(vec![
                p.id.clone(),
                verification(app, &p.verification).into(),
            ])
        })
        .collect();
    let mut state = TableState::default().with_selected(Some(app.profile_cursor));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Percentage(65), Constraint::Min(10)])
            .header(header(
                crate::theme::Theme::named(&app.config.settings.theme),
                &[app.text("profile"), app.text("verification")],
            ))
            .block(crate::chrome::panel(theme).title(app.text("profiles")))
            .row_highlight_style(Style::default().bg(theme.selection).fg(theme.foreground)),
        parts[0],
        &mut state,
    );
    table_hits(
        frame,
        app,
        parts[0],
        state.offset(),
        app.registry
            .profiles
            .keys()
            .map(|id| Target::Profile(id.clone()))
            .collect(),
        Pane::Profiles,
    );
    app.hits
        .push(Hit::new(parts[1], Target::Scroll(Pane::Detail)));
    let text = app
        .registry
        .profiles
        .values()
        .nth(app.profile_cursor)
        .map(|p| {
            let fan = p
                .fan
                .as_ref()
                .map(|f| {
                    format!(
                        "{}\n{}: {}",
                        f.zones
                            .iter()
                            .map(|z| format!("{}={}", z.id, z.name))
                            .collect::<Vec<_>>()
                            .join(", "),
                        app.text("raw"),
                        if f.raw_values.is_empty() {
                            if f.percent_values.is_empty() {
                                format!("{}..{}%", f.min_percent, f.max_percent)
                            } else {
                                f.percent_values
                                    .iter()
                                    .map(|v| format!("{v}%"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            }
                        } else {
                            f.raw_values
                                .iter()
                                .map(|v| format!("{v:#04x}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    )
                })
                .unwrap_or_else(|| app.text("monitor-only").into());
            format!(
                "{}\n{}\n\n{}\n\n{}\n\n{}:\n{}\n\n{}:\n{}\n\n{}:\n{}",
                p.vendor,
                p.description,
                p.notes,
                fan,
                app.text("sources"),
                p.sources.join("\n"),
                app.text("factory-credentials"),
                p.default_credentials
                    .iter()
                    .map(|c| format!(
                        "{} / {} — {}\n{}",
                        c.username,
                        c.password.as_deref().unwrap_or("—"),
                        c.note,
                        c.source
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                app.text("server"),
                app.catalog
                    .models
                    .values()
                    .filter(|m| m.profile.as_deref() == Some(p.id.as_str()))
                    .map(|m| format!("{} ({})", m.name, m.id))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
        .unwrap_or_default();
    let (position, hits) = crate::chrome::paragraph(
        frame,
        parts[1],
        &text,
        app.text("profiles"),
        app.detail_scroll,
        Pane::Detail,
        theme,
    );
    app.detail_scroll = position;
    app.hits.extend(hits);
}
fn logs(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    app.hits.push(Hit::new(area, Target::Scroll(Pane::Logs)));
    let text = app
        .logs
        .iter()
        .map(|s| app.locales.render(&app.config.settings.language, s))
        .collect::<Vec<_>>()
        .join("\n");
    let (position, hits) = crate::chrome::paragraph(
        frame,
        area,
        &text,
        app.text("activity-redacted-x-exports-diagnostics"),
        app.log_scroll,
        Pane::Logs,
        theme,
    );
    app.log_scroll = position;
    app.hits.extend(hits);
}

fn button_bar(frame: &mut Frame, area: Rect, app: &mut App, buttons: Vec<(String, Target)>) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    app.interaction.menu_items = buttons.clone();
    app.interaction.actions = buttons.iter().map(|(_, t)| t.clone()).collect();
    let labels = buttons
        .iter()
        .map(|(text, target)| format!("[{} {text}]", app.target_label(target)))
        .collect::<Vec<_>>();
    let mut required_rows = 1;
    let mut used = 0;
    for label in &labels {
        let width = (label.width() as u16).min(area.width);
        if used + width > area.width {
            required_rows += 1;
            used = 0;
        }
        used += width + 1;
    }
    let overflow = required_rows > area.height;
    let overflow_width = if overflow { area.width.min(4) } else { 0 };
    if overflow_width > 0 && area.height > 0 {
        let more = Rect::new(area.right() - overflow_width, area.y, overflow_width, 1);
        frame.render_widget(Paragraph::new(" ⋯ ").style(theme.focus()), more);
        app.hits.push(Hit::new(more, Target::Overflow));
    }
    let area = Rect {
        width: area.width.saturating_sub(overflow_width),
        ..area
    };
    let hint = if overflow && area.height > 1 {
        Some(Rect::new(area.x, area.bottom() - 1, area.width, 1))
    } else {
        None
    };
    let area = Rect {
        height: area.height.saturating_sub(u16::from(hint.is_some())),
        ..area
    };
    let fits = |start: usize, end: usize| {
        let mut x = 0;
        let mut rows = 1;
        for label in &labels[start..end] {
            let width = (label.width() as u16).min(area.width);
            if x + width > area.width {
                rows += 1;
                x = 0;
            }
            x += width + 1;
        }
        rows <= area.height.max(1)
    };
    let focus = app
        .interaction
        .action_focus
        .unwrap_or(0)
        .min(buttons.len().saturating_sub(1));
    let start = if fits(0, buttons.len()) {
        0
    } else {
        let mut start = app.interaction.action_offset.min(focus);
        while start < focus && !fits(start, focus + 1) {
            start += 1;
        }
        start
    };
    app.interaction.action_offset = start;
    if let Some(hint) = hint {
        frame.render_widget(
            Paragraph::new(format!("F8 ←/→ Enter · {}/{}", focus + 1, buttons.len())),
            hint,
        );
    }
    let mut x = area.x;
    let mut y = area.y;
    for (index, ((_, target), label)) in buttons.into_iter().zip(labels).enumerate().skip(start) {
        let width = (label.width() as u16).min(area.width);
        if x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() {
            break;
        }
        let rect = Rect::new(x, y, width, 1);
        let disabled = !app.target_reason(&target).is_empty();
        let style = if disabled {
            Style::default()
                .fg(theme.muted)
                .bg(if app.interaction.action_focus == Some(index) {
                    theme.selection
                } else {
                    theme.surface
                })
        } else if app.interaction.action_focus == Some(index) {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent)
        };
        frame.render_widget(Paragraph::new(label).style(style), rect);
        app.hits.push(Hit::new(rect, target));
        x = x.saturating_add(width + 1);
    }
}

fn table_hits(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    offset: usize,
    items: Vec<Target>,
    pane: Pane,
) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let count = items.len();
    app.hits.push(Hit::new(area, Target::Scroll(pane)));
    for (row, target) in items
        .into_iter()
        .skip(offset)
        .take(area.height.saturating_sub(3).into())
        .enumerate()
    {
        let rect = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(2 + row as u16),
            area.width.saturating_sub(2),
            1,
        );
        app.hits.push(Hit::new(rect, target.clone()));
        let checkbox = match target {
            Target::Server(id, _) => Some(Target::Server(id, true)),
            Target::Discovery(host, _) => Some(Target::Discovery(host, true)),
            _ => None,
        };
        if let Some(target) = checkbox {
            app.hits.push(Hit::new(
                Rect {
                    width: rect.width.min(3),
                    ..rect
                },
                target,
            ));
        }
    }
    let position = match pane {
        Pane::Fleet => app.cursor,
        Pane::Discovery => app.scan_cursor,
        Pane::Profiles => app.profile_cursor,
        _ => offset,
    };
    if count > usize::from(area.height.saturating_sub(3)) {
        app.hits.extend(crate::chrome::scrollbar(
            frame,
            Rect::new(
                area.right().saturating_sub(1),
                area.y.saturating_add(2),
                1.min(area.width),
                area.height.saturating_sub(3),
            ),
            position,
            count.saturating_sub(1),
            pane,
            theme,
        ));
    }
}
fn draw_form(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    use crate::form::FieldType;
    let Some(Modal::Form(form)) = &app.modal else {
        return;
    };
    let title = form.title.clone();
    let block = crate::chrome::panel(theme)
        .title(title)
        .title_bottom(" Tab/Shift+Tab · ↑↓ · F2 · Esc · F8 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let has_error = !form.error.is_empty() || !form.fields[form.focus].error.is_empty();
    let parts = Layout::vertical([
        Constraint::Length(if inner.height >= 8 { 2 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(if has_error && inner.height >= 5 { 2 } else { 0 }),
    ])
    .split(inner);
    let help = app.text("form-help").to_owned();
    frame.render_widget(Paragraph::new(help).wrap(Wrap { trim: false }), parts[0]);
    let height = |i: usize| {
        if matches!(form.fields[i].kind, FieldType::Multiline) {
            6u16
        } else {
            3u16
        }
    };
    let mut start = 0;
    while start < form.focus && (start..=form.focus).map(height).sum::<u16>() > parts[1].height {
        start += 1;
    }
    let mut y = parts[1].y;
    for (index, field) in form.fields.iter().enumerate().skip(start) {
        if y >= parts[1].bottom() {
            break;
        }
        let rect = Rect::new(
            parts[1].x,
            y,
            parts[1].width,
            height(index).min(parts[1].bottom() - y),
        );
        let hint = match &field.kind {
            FieldType::Toggle => app.text("form-toggle").into(),
            FieldType::Choice(options) => format!(
                "{} {}/{}",
                app.text("form-choice"),
                options
                    .iter()
                    .position(|v| v == &field.value())
                    .unwrap_or(0)
                    + 1,
                options.len()
            ),
            FieldType::Secret => format!("F6: {}", app.text("form-reveal")),
            FieldType::Number { min, max } => format!("{min}–{max}"),
            FieldType::Tags => app.text("form-tags").into(),
            FieldType::Multiline => app.text("form-lines").into(),
            _ => String::new(),
        };
        let label = if field.path.first().is_some_and(|p| p == "keys") {
            format!(
                "{} ({})",
                app.text(&field.label),
                field.path.last().unwrap()
            )
        } else {
            app.text(&field.label).into()
        };
        let style = if !field.error.is_empty() {
            Style::default().fg(theme.danger)
        } else if index == form.focus {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let block = crate::chrome::panel(theme)
            .title(format!(
                "{} / {} · {}/{}",
                app.text(field.group),
                label,
                index + 1,
                form.fields.len()
            ))
            .title_bottom(hint)
            .border_style(style)
            .style(if index == form.focus {
                Style::default().bg(theme.selection)
            } else {
                theme.surface()
            });
        let content = block.inner(rect);
        frame.render_widget(block, rect);
        app.hits.push(Hit::new(rect, Target::Field(index)));
        if content.width > 0 && content.height > 0 {
            let text = if matches!(field.kind, FieldType::Toggle) {
                if field.value() == "true" {
                    "[✓]".into()
                } else {
                    "[ ]".into()
                }
            } else if matches!(field.kind, FieldType::Choice(_)) && field.value().is_empty() {
                app.text("automatic").into()
            } else {
                field.display()
            };
            let prefix: String = if matches!(field.kind, FieldType::Secret) && !field.revealed {
                "•".repeat(field.input.col)
            } else {
                field.input.lines[field.input.row]
                    .chars()
                    .take(field.input.col)
                    .collect()
            };
            let cursor_x = prefix.width();
            let left = cursor_x.saturating_sub(content.width.saturating_sub(1).into());
            let top = field
                .input
                .row
                .saturating_sub(usize::from(content.height.saturating_sub(1)));
            frame.render_widget(
                Paragraph::new(text).scroll((top as u16, left as u16)),
                content,
            );
            app.hits.push(Hit::new(content, Target::Field(index)));
            if index == form.focus
                && !matches!(field.kind, FieldType::Choice(_) | FieldType::Toggle)
            {
                frame.set_cursor_position((
                    content.x + (cursor_x - left) as u16,
                    content.y + (field.input.row - top) as u16,
                ));
            }
        }
        y = y.saturating_add(rect.height);
    }
    app.hits.extend(crate::chrome::scrollbar(
        frame,
        Rect::new(
            area.right().saturating_sub(1),
            parts[1].y,
            1.min(area.width),
            parts[1].height,
        ),
        form.focus,
        form.fields.len().saturating_sub(1),
        Pane::Modal,
        theme,
    ));
    let error = if form.error.is_empty() {
        &form.fields[form.focus].error
    } else {
        &form.error
    };
    frame.render_widget(
        Paragraph::new(app.text(error).to_owned())
            .style(Style::default().fg(theme.danger))
            .wrap(Wrap { trim: false }),
        parts[2],
    );
}
fn modal(frame: &mut Frame, app: &mut App) {
    let theme = crate::theme::Theme::named(&app.config.settings.theme);
    let area = frame.area();
    let popup = Rect {
        x: area.x + area.width / 20,
        y: area.y + area.height / 12,
        width: area.width.saturating_sub(area.width / 10),
        height: area.height.saturating_sub(area.height / 6),
    };
    crate::chrome::popup(frame, popup, theme);
    let buttons_area = Rect::new(
        popup.x,
        popup.bottom().saturating_sub(2),
        popup.width,
        2.min(popup.height),
    );
    let popup = Rect {
        height: popup.height.saturating_sub(2),
        ..popup
    };
    app.hits.push(Hit::new(popup, Target::Scroll(Pane::Modal)));
    let primary = if matches!(app.modal, Some(Modal::Editor { .. } | Modal::Form(_))) {
        KeyCode::F(2)
    } else {
        KeyCode::Enter
    };
    let mut buttons = vec![
        (app.text("confirm").into(), Target::Key(primary)),
        (app.text("cancel").into(), Target::Key(KeyCode::Esc)),
    ];
    if matches!(&app.modal, Some(Modal::Form(f)) if f.advanced) || app.advanced_form.is_some() {
        buttons.push((app.text("form-advanced").into(), Target::Key(KeyCode::F(7))));
    }
    if matches!(&app.modal, Some(Modal::Form(f)) if matches!(f.fields[f.focus].kind, crate::form::FieldType::Secret))
    {
        buttons.push((app.text("form-reveal").into(), Target::Key(KeyCode::F(6))));
    }
    if matches!(app.modal, Some(Modal::ModelChoice { .. })) {
        buttons = vec![
            (app.text("confirm").into(), Target::Key(KeyCode::Enter)),
            (app.text("unknown").into(), Target::Key(KeyCode::F(5))),
            (app.text("skip").into(), Target::Key(KeyCode::F(6))),
        ];
    }
    let languages = app
        .locales
        .packs
        .iter()
        .map(|(id, p)| format!("{} ({id})", p.name))
        .collect::<Vec<_>>();
    let model_options = match &app.modal {
        Some(Modal::ModelChoice { query, .. }) => app.model_options(query),
        _ => vec![],
    };
    let model_rows = model_options
        .iter()
        .map(|id| {
            let m = &app.catalog.models[id];
            let candidate = app
                .pending
                .first()
                .and_then(|p| p.candidates.iter().find(|c| &c.id == id));
            format!(
                "{} | {} | {}",
                m.name,
                app.pending
                    .first()
                    .map(|p| app.capability(&p.server, &p.identity, m.profile.as_deref()))
                    .unwrap_or_else(|| app.text("monitor-only").into()),
                candidate.map(|c| c.evidence.join("; ")).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let model_title = app.text("resolve-model").to_owned();
    let labels = [
        "preview",
        "duplicates",
        "invalid-lines",
        "save-plaintext",
        "one-credential",
        "confirm",
        "cancel",
        "continue",
        "search",
        "unknown",
        "skip",
        "profile",
        "firmware",
        "selected",
        "remove-server",
        "safety",
    ]
    .into_iter()
    .map(|k| (k, app.text(k).to_owned()))
    .collect::<std::collections::BTreeMap<_, _>>();
    let evidence = app
        .pending
        .first()
        .map(|p| {
            format!(
                "{} | raw FRU: {} | firmware: {}\n{}",
                p.server.endpoint(),
                p.identity.model,
                p.identity.firmware,
                p.error.as_deref().unwrap_or("")
            )
        })
        .unwrap_or_default();
    let import_text = if let Some(Modal::Import { servers, .. }) = &app.modal {
        servers
            .iter()
            .map(|s| {
                let empty = bmcforge_core::DeviceIdentity::default();
                let identity = s.identity_evidence.as_ref().unwrap_or(&empty);
                format!(
                    "{} | {} | {} | {} | {}",
                    s.endpoint(),
                    s.model_id.as_deref().unwrap_or(&labels["unknown"]),
                    s.identification_error
                        .as_deref()
                        .unwrap_or(if s.model_id.is_some() {
                            &labels["selected"]
                        } else {
                            &labels["unknown"]
                        }),
                    app.capability(s, identity, s.profile.as_deref()),
                    identity.model
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    if let Some(Modal::Actions { items, cursor, .. }) = &app.modal {
        let rows = items.iter().map(|(label, target)| {
            let reason = app.target_reason(target);
            Row::new(vec![
                label.clone(),
                app.target_label(target),
                reason.clone(),
            ])
            .style(Style::default().fg(if reason.is_empty() {
                theme.foreground
            } else {
                theme.muted
            }))
        });
        let mut state = TableState::default().with_selected(Some(*cursor));
        frame.render_stateful_widget(
            Table::new(
                rows,
                [
                    Constraint::Percentage(42),
                    Constraint::Length(10),
                    Constraint::Min(1),
                ],
            )
            .block(crate::chrome::panel(theme).title(app.text("ui-more")))
            .row_highlight_style(theme.selected()),
            popup,
            &mut state,
        );
        for (row, index) in (state.offset()..items.len())
            .take(popup.height.saturating_sub(2).into())
            .enumerate()
        {
            app.hits.push(Hit::new(
                Rect::new(
                    popup.x + 1,
                    popup.y + 1 + row as u16,
                    popup.width.saturating_sub(2),
                    1,
                ),
                Target::ActionRow(index),
            ));
        }
        app.hits.extend(crate::chrome::scrollbar(
            frame,
            Rect::new(
                popup.right().saturating_sub(1),
                popup.y + 1,
                1,
                popup.height.saturating_sub(2),
            ),
            *cursor,
            items.len().saturating_sub(1),
            Pane::Modal,
            theme,
        ));
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    if let Some(Modal::FieldChoice {
        form,
        query,
        cursor,
    }) = &app.modal
    {
        let options = form.choice_options(query);
        let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(popup);
        let field = &form.fields[form.focus];
        frame.render_widget(
            Paragraph::new(format!(
                "{}: {query} · ↑↓ Enter · Esc · F8",
                app.text("search")
            ))
            .block(crate::chrome::panel(theme).title(app.text(&field.label))),
            parts[0],
        );
        let mut state = TableState::default().with_selected(Some(*cursor));
        frame.render_stateful_widget(
            Table::new(
                options.iter().map(|v| {
                    Row::new([if v.is_empty() {
                        app.text("automatic").to_owned()
                    } else {
                        v.clone()
                    }])
                }),
                [Constraint::Min(1)],
            )
            .row_highlight_style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            parts[1],
            &mut state,
        );
        for (row, index) in (state.offset()..options.len())
            .take(parts[1].height.into())
            .enumerate()
        {
            app.hits.push(Hit::new(
                Rect::new(parts[1].x, parts[1].y + row as u16, parts[1].width, 1),
                Target::ChoiceRow(index),
            ));
        }
        app.hits.extend(crate::chrome::scrollbar(
            frame,
            parts[1],
            *cursor,
            options.len().saturating_sub(1),
            Pane::Modal,
            theme,
        ));
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    if matches!(app.modal, Some(Modal::Form(_))) {
        draw_form(frame, popup, app);
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    if let Some(Modal::Commands { query, cursor }) = &app.modal {
        let entries = app.commands(query);
        let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(popup);
        frame.render_widget(
            Paragraph::new(format!("{}  › {}", app.text("command-palette"), query))
                .block(Block::default().borders(Borders::BOTTOM)),
            parts[0],
        );
        let rows = entries.iter().map(|entry| {
            Row::new(vec![
                entry.label.clone(),
                app.target_label(&entry.target),
                entry.reason.clone(),
            ])
            .style(if entry.reason.is_empty() {
                Style::default().fg(theme.foreground)
            } else {
                Style::default().fg(theme.muted)
            })
        });
        let mut state = TableState::default().with_selected(if entries.is_empty() {
            None
        } else {
            Some((*cursor).min(entries.len() - 1))
        });
        frame.render_stateful_widget(
            Table::new(
                rows,
                [
                    Constraint::Percentage(45),
                    Constraint::Length(10),
                    Constraint::Min(1),
                ],
            )
            .row_highlight_style(theme.selected()),
            parts[1],
            &mut state,
        );
        if entries.is_empty() {
            frame.render_widget(
                Paragraph::new(app.text("command-no-results"))
                    .style(Style::default().fg(theme.muted)),
                parts[1],
            );
        }
        for row in 0..parts[1].height {
            let index = state.offset() + usize::from(row);
            if index < entries.len() {
                app.hits.push(Hit::new(
                    Rect::new(parts[1].x, parts[1].y + row, parts[1].width, 1),
                    Target::CommandRow(index),
                ));
            }
        }
        app.hits.extend(crate::chrome::scrollbar(
            frame,
            parts[1],
            *cursor,
            entries.len().saturating_sub(1),
            Pane::Modal,
            theme,
        ));
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    let Some(modal) = &mut app.modal else {
        return;
    };
    match modal {
        Modal::Commands { .. } | Modal::Actions { .. } => unreachable!(),

        Modal::Form(_) | Modal::FieldChoice { .. } => unreachable!(),
        Modal::Languages { cursor } => {
            let mut state = TableState::default().with_selected(Some(*cursor));
            frame.render_stateful_widget(
                Table::new(
                    languages.iter().map(|s| Row::new([s.clone()])),
                    [Constraint::Min(1)],
                )
                .block(crate::chrome::panel(theme).title("🌐 ↑↓ Enter · F8"))
                .row_highlight_style(theme.selected()),
                popup,
                &mut state,
            );
            for (row, index) in (state.offset()..languages.len())
                .take(popup.height.saturating_sub(2).into())
                .enumerate()
            {
                app.hits.push(Hit::new(
                    Rect::new(
                        popup.x + 1,
                        popup.y + 1 + row as u16,
                        popup.width.saturating_sub(2),
                        1,
                    ),
                    Target::LanguageRow(index),
                ));
            }
            app.hits.extend(crate::chrome::scrollbar(
                frame,
                Rect::new(
                    popup.right().saturating_sub(1),
                    popup.y + 1,
                    1.min(popup.width),
                    popup.height.saturating_sub(2),
                ),
                *cursor,
                languages.len().saturating_sub(1),
                Pane::Modal,
                theme,
            ));
        }
        Modal::ModelChoice { query, cursor } => {
            let parts = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).split(popup);
            frame.render_widget(
                Paragraph::new(format!(
                    "{evidence}\n{}: {query} | F5: {} | F6: {}",
                    labels["search"], labels["unknown"], labels["skip"]
                ))
                .block(crate::chrome::panel(theme).title(model_title))
                .wrap(Wrap { trim: false }),
                parts[0],
            );
            let mut state = TableState::default().with_selected(Some(*cursor));
            frame.render_stateful_widget(
                Table::new(
                    model_rows.iter().map(|s| Row::new([s.clone()])),
                    [Constraint::Min(1)],
                )
                .row_highlight_style(theme.selected()),
                parts[1],
                &mut state,
            );
            for (row, id) in model_options
                .iter()
                .skip(state.offset())
                .take(parts[1].height.into())
                .enumerate()
            {
                app.hits.push(Hit::new(
                    Rect::new(parts[1].x, parts[1].y + row as u16, parts[1].width, 1),
                    Target::ModelRow(id.clone()),
                ));
            }
            app.hits.extend(crate::chrome::scrollbar(
                frame,
                parts[1],
                *cursor,
                model_options.len().saturating_sub(1),
                Pane::Modal,
                theme,
            ));
        }
        Modal::Import {
            servers,
            authenticate,
            duplicates,
        } => {
            let text = format!(
                "{} | {}: {}\n{}\n\n{}",
                servers.len(),
                labels["duplicates"],
                duplicates,
                import_text,
                if *authenticate {
                    &labels["one-credential"]
                } else {
                    &labels["save-plaintext"]
                }
            );
            let (position, hits) = crate::chrome::paragraph(
                frame,
                popup,
                &text,
                &labels["preview"],
                app.modal_scroll,
                Pane::Modal,
                theme,
            );
            app.modal_scroll = position;
            app.hits.extend(hits);
        }
        Modal::Editor {
            title,
            editor,
            error,
            ..
        } => {
            let block = crate::chrome::panel(theme)
                .title(title.clone())
                .title_bottom(format!(
                    " F7: ↩ | F8: actions | F2 / Ctrl+Enter: {} | Esc: {} ",
                    labels["confirm"], labels["cancel"]
                ));
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
            let height = usize::from(parts[0].height.max(1));
            if editor.row < editor.scroll {
                editor.scroll = editor.row;
            }
            if editor.row >= editor.scroll + height {
                editor.scroll = editor.row + 1 - height;
            }
            let prefix: String = editor.lines[editor.row].chars().take(editor.col).collect();
            let width = UnicodeWidthStr::width(prefix.as_str());
            let horizontal = width.saturating_sub(usize::from(parts[0].width.saturating_sub(1)));
            app.hits.push(Hit::new(
                parts[0],
                Target::Editor {
                    top: editor.scroll,
                    left: horizontal,
                },
            ));
            frame.render_widget(
                Paragraph::new(editor.text()).scroll((editor.scroll as u16, horizontal as u16)),
                parts[0],
            );
            frame.render_widget(
                Paragraph::new(error.clone())
                    .style(Style::default().fg(theme.danger))
                    .wrap(Wrap { trim: false }),
                parts[1],
            );
            if parts[0].height > 0 && parts[0].width > 0 {
                frame.set_cursor_position((
                    parts[0].x + (width - horizontal) as u16,
                    parts[0].y + (editor.row - editor.scroll) as u16,
                ));
            }
            app.hits.extend(crate::chrome::scrollbar(
                frame,
                Rect::new(
                    popup.right().saturating_sub(1),
                    parts[0].y,
                    1.min(popup.width),
                    parts[0].height,
                ),
                editor.row,
                editor.lines.len().saturating_sub(1),
                Pane::Modal,
                theme,
            ));
        }
        Modal::Preview {
            hosts,
            discovery,
            duplicates,
            errors,
        } => {
            let large = *discovery && hosts.len() > app.config.settings.scan_confirm_above;
            let text = format!(
                "{} | {}: {} | {}: {}{}\n\n{}\n{}\n\nEnter: {} | Esc: {}",
                hosts.len(),
                labels["duplicates"],
                duplicates,
                labels["invalid-lines"],
                errors.len(),
                if large { " ⚠" } else { "" },
                hosts.join("\n"),
                errors.join("\n"),
                labels["continue"],
                labels["cancel"]
            );
            let (position, hits) = crate::chrome::paragraph(
                frame,
                popup,
                &text,
                &labels["preview"],
                app.modal_scroll,
                Pane::Modal,
                theme,
            );
            app.modal_scroll = position;
            app.hits.extend(hits);
        }
        Modal::Confirm { title, text, .. } => {
            let mut lines = vec![
                Line::styled(
                    title.clone(),
                    Style::default()
                        .fg(theme.warning)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::from(""),
            ];
            lines.extend(text.lines().map(|line| Line::from(line.to_owned())));
            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Enter: {} | Esc: {} | PgUp/PgDn",
                labels["confirm"], labels["cancel"]
            )));
            let inner = crate::chrome::panel(theme).inner(popup);
            let paragraph = Paragraph::new(lines)
                .block(crate::chrome::panel(theme).title(labels["preview"].clone()))
                .wrap(Wrap { trim: false });
            let maximum = paragraph
                .line_count(popup.width)
                .saturating_sub(usize::from(popup.height))
                .min(u16::MAX as usize);
            app.modal_scroll = usize::from(app.modal_scroll).min(maximum) as u16;
            frame.render_widget(paragraph.scroll((app.modal_scroll, 0)), popup);
            app.hits.extend(crate::chrome::scrollbar(
                frame,
                Rect::new(
                    popup.right().saturating_sub(1),
                    inner.y,
                    1.min(popup.width),
                    inner.height,
                ),
                usize::from(app.modal_scroll),
                maximum,
                Pane::Modal,
                theme,
            ));
        }
        Modal::Info(text) => {
            let (position, hits) = crate::chrome::paragraph(
                frame,
                popup,
                text,
                " BMCForge ",
                app.modal_scroll,
                Pane::Modal,
                theme,
            );
            app.modal_scroll = position;
            app.hits.extend(hits);
        }
        Modal::Remove { name, .. } => {
            let text = format!("{}\n{name}\n{}", labels["remove-server"], labels["safety"]);
            let (position, hits) = crate::chrome::paragraph(
                frame,
                popup,
                &text,
                &labels["confirm"],
                app.modal_scroll,
                Pane::Modal,
                theme,
            );
            app.modal_scroll = position;
            app.hits.extend(hits);
        }
    }
    button_bar(frame, buttons_area, app, buttons);
}

#[cfg(test)]
mod tests {

    use super::*;
    use bmcforge_core::config::Config;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn operation_status_stays_visible_when_shortcuts_are_wider_than_terminal() {
        let mut app = App::new(
            Config::default(),
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        );
        app.status = "RESTORE FAILED: check device".into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("RESTORE FAILED: check device"));
    }
    #[test]
    fn batch_confirmation_renders_separate_server_lines_and_scrolls() {
        let mut app = App::new(
            Config::default(),
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        );
        app.modal = Some(Modal::Confirm {
            title: "control".into(),
            text: (0..30)
                .map(|i| format!("server-{i:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
            control: vec![],
            manual: None,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        app.modal_scroll = 20;
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("server-20"));
        assert!(!text.contains("server-00"));
    }
    #[test]
    fn renders_every_page_in_both_languages_and_small_terminals() {
        for language in [
            "en", "zh-CN", "zh-TW", "ja", "ko", "de", "fr", "es", "pt-BR", "it", "ru",
        ] {
            for (width, height) in [(120, 35), (60, 18), (20, 8)] {
                let mut cfg = Config::default();
                cfg.settings.language = language.into();
                cfg.servers = bmcforge_ipmi::mock::servers();
                let mut app = App::new(
                    cfg,
                    "unused".into(),
                    bmcforge_profiles::Registry::load(None).unwrap(),
                    true,
                );
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                for page in 0..5 {
                    app.page = page;
                    terminal.draw(|f| draw(f, &mut app)).unwrap();
                }
                app.modal = Some(Modal::Editor {
                    title: "服务器密码".into(),
                    kind: crate::app::EditKind::Search,
                    editor: crate::editor::Editor::new("服务器\npassword = \"demo\""),
                    error: String::new(),
                });
                terminal.draw(|f| draw(f, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                assert!(buffer.content().iter().any(|c| c.symbol() == "服"));
            }
        }
    }
    fn click(app: &mut App, target: impl Fn(&Target) -> bool) -> Vec<crate::app::Action> {
        let hit = app
            .hits
            .iter()
            .find(|h| target(&h.target))
            .expect("visible clickable control");
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: hit.area.x + hit.area.width / 2,
            row: hit.area.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        app.mouse(event)
    }
    #[test]
    fn mouse_rows_use_actual_offsets_double_click_and_modal_isolation() {
        let mut cfg = Config::default();
        let template = bmcforge_ipmi::mock::servers().remove(0);
        cfg.servers = (0..40)
            .map(|i| bmcforge_core::config::Server {
                id: format!("id-{i:02}"),
                name: format!("host-{i:02}"),
                host: format!("192.0.2.{}", i + 1),
                ..template.clone()
            })
            .collect();
        let mut app = App::new(
            cfg,
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
        app.cursor = 35;
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        click(
            &mut app,
            |t| matches!(t,Target::Server(id,true) if id=="id-35"),
        );
        assert!(app.selected.contains("id-35"));
        click(
            &mut app,
            |t| matches!(t,Target::Server(id,false) if id=="id-35"),
        );
        click(
            &mut app,
            |t| matches!(t,Target::Server(id,false) if id=="id-35"),
        );
        assert_eq!(app.page, 1);
        // Even before a redraw, a newly opened modal must reject stale background hits.
        app.modal = Some(Modal::Info("test".into()));
        let old_page = app.page;
        click(&mut app, |t| matches!(t, Target::Page(4)));
        assert_eq!(app.page, old_page);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(
            !app.hits
                .iter()
                .any(|h| matches!(h.target, Target::Page(_) | Target::Server(..)))
        );
        click(&mut app, |t| matches!(t, Target::Key(KeyCode::Esc)));
        assert!(app.modal.is_none());
    }
    #[test]
    fn resized_editor_hits_and_remapped_shortcuts_keep_mouse_actions() {
        let mut app = App::new(
            Config::default(),
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        );
        for width in [80, 20] {
            app.modal = Some(Modal::Editor {
                title: "test".into(),
                kind: crate::app::EditKind::Search,
                editor: crate::editor::Editor::new("A中文B"),
                error: String::new(),
            });
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let hit = app
                .hits
                .iter()
                .find(|h| matches!(h.target, Target::Editor { .. }))
                .unwrap();
            let event = crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: hit.area.x + 3,
                row: hit.area.y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };
            app.mouse(event);
            assert!(matches!(app.modal,Some(Modal::Editor{ref editor,..}) if editor.col==2));
        }
        app.modal = None;
        app.page = 2;
        app.config.settings.keys.insert("quit".into(), "A".into());
        app.discovery
            .push(bmcforge_ipmi::discovery::DiscoveryResult {
                host: "192.0.2.20".into(),
                status: bmcforge_ipmi::discovery::ProbeStatus::IpmiCandidate,
            });
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(click(&mut app, |t| matches!(t, Target::Key(KeyCode::Char('A')))).is_empty());
        assert!(app.discovery_selected.contains("192.0.2.20"));
    }
    #[test]
    fn mouse_local_scroll_language_switch_and_control_preview() {
        let cfg = Config {
            servers: bmcforge_ipmi::mock::servers(),
            ..Default::default()
        };
        let mut app = App::new(
            cfg,
            "unused".into(),
            bmcforge_profiles::Registry::load(None).unwrap(),
            true,
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let hit = app
            .hits
            .iter()
            .find(|h| matches!(h.target, Target::Scroll(Pane::Detail)))
            .unwrap();
        let wheel = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: hit.area.x,
            row: hit.area.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        app.mouse(wheel);
        assert_eq!(app.detail_scroll, 1);
        assert_eq!(app.cursor, 0);
        click(&mut app, |t| matches!(t, Target::Language));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let zh = app
            .locales
            .packs
            .keys()
            .position(|id| id == "zh-CN")
            .unwrap();
        click(&mut app, |t| matches!(t,Target::LanguageRow(i) if *i==zh));
        assert_eq!(app.config.settings.language, "zh-CN");
        app.log(crate::i18n::Message::new("ready"));
        assert_eq!(
            app.locales.render("ja", app.logs.back().unwrap()),
            "準備完了"
        );
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(click(&mut app, |t| matches!(t,Target::Shortcut(s) if s=="manual")).is_empty());
        assert!(matches!(app.modal, Some(Modal::Form(_))));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(click(&mut app, |t| matches!(t, Target::Key(KeyCode::F(2)))).is_empty());
        assert!(matches!(app.modal, Some(Modal::Confirm { .. })));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(matches!(
            click(&mut app, |t| matches!(t, Target::Key(KeyCode::Enter))).as_slice(),
            [crate::app::Action::Send(
                _,
                crate::runtime::Command::ManualValue(_, _, _)
            )]
        ));
    }
    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    fn key(app: &mut App, code: KeyCode) {
        app.input(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    fn demo() -> App {
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
    #[test]
    fn hover_click_expiry_resize_and_modal_changes_are_isolated() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        use std::time::{Duration, Instant};
        let mut app = demo();
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let hit = app
            .hits
            .iter()
            .find(|h| matches!(h.target, Target::Page(4)))
            .unwrap()
            .clone();
        let event = MouseEvent {
            kind: MouseEventKind::Moved,
            column: hit.area.x,
            row: hit.area.y,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(event);
        assert_eq!(app.page, 0);
        assert_eq!(app.interaction.hover, Some(Target::Page(4)));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(hit.area.x, hit.area.y)].bg,
            crate::theme::Theme::named("dark").accent
        );
        app.mouse(MouseEvent {
            column: 119,
            row: 34,
            ..event
        });
        assert!(app.interaction.hover.is_none());
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            ..event
        });
        assert_eq!(app.page, 4);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(hit.area.x, hit.area.y)].bg,
            crate::theme::Theme::named("dark").warning
        );
        app.interaction.pressed =
            Some((Target::Page(4), Instant::now() - Duration::from_millis(151)));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_ne!(
            terminal.backend().buffer()[(hit.area.x, hit.area.y)].bg,
            crate::theme::Theme::named("dark").warning
        );
        app.mouse(event);
        app.modal = Some(Modal::Info("test".into()));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(app.interaction.hover.is_none());
        app.mouse(event);
        assert!(!matches!(app.interaction.hover, Some(Target::Page(_))));
        app.modal = None;
        app.page = 0;
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        app.mouse(event);
        let mut small = Terminal::new(TestBackend::new(30, 12)).unwrap();
        small.draw(|f| draw(f, &mut app)).unwrap();
        assert!(app.interaction.hover.is_none());
    }
    #[test]
    fn every_overflow_action_is_keyboard_reachable_with_remapped_labels() {
        let mut app = demo();
        app.config.settings.keys.insert("quit".into(), "Q".into());
        let mut terminal = Terminal::new(TestBackend::new(30, 16)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::F(8));
        let count = app.interaction.actions.len();
        for index in 0..count {
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let target = app.interaction.actions[index].clone();
            assert!(app.hits.iter().any(|h| h.target == target));
            if matches!(&target, Target::Shortcut(s) if s == "quit") {
                assert!(buffer_text(&terminal).contains(&format!("Q {}", app.text("quit"))));
            }
            key(&mut app, KeyCode::Right);
        }
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.interaction.action_focus, None);
        // The fixed select-all action remains separate from a legacy conflicting user shortcut.
        app.page = 2;
        app.config.settings.keys.insert("quit".into(), "A".into());
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::F(8));
        let index = app
            .interaction
            .actions
            .iter()
            .position(|t| matches!(t, Target::Key(KeyCode::Char('A'))))
            .unwrap();
        app.interaction.action_focus = Some(index);
        assert!(
            app.input(crossterm::event::KeyEvent::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE
            ))
            .is_empty()
        );
    }
    #[test]
    fn forms_render_masked_and_all_fields_remain_visible_when_focused() {
        for language in [
            "en", "zh-CN", "zh-TW", "ja", "ko", "de", "fr", "es", "pt-BR", "it", "ru",
        ] {
            for theme in ["dark", "light"] {
                for (width, height) in [(120, 35), (60, 18), (20, 8)] {
                    let mut app = demo();
                    app.config.settings.language = language.into();
                    app.config.settings.theme = theme.into();
                    app.config.servers[0].credentials.password = "secret-marker".into();
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    key(&mut app, KeyCode::Char('e'));
                    let count = if let Some(Modal::Form(f)) = &app.modal {
                        f.fields.len()
                    } else {
                        panic!("form");
                    };
                    for index in 0..count {
                        if let Some(Modal::Form(f)) = &mut app.modal {
                            f.focus = index;
                        }
                        terminal.draw(|f| draw(f, &mut app)).unwrap();
                        assert!(!buffer_text(&terminal).contains("secret-marker"));
                        if width >= 20 {
                            assert!(app.hits.iter().any(|h| h.target == Target::Field(index)));
                        }
                    }
                    if width >= 60 {
                        let Some(Modal::Form(f)) = &mut app.modal else {
                            unreachable!()
                        };
                        f.focus = f
                            .fields
                            .iter()
                            .position(|f| f.path.last().unwrap() == "password")
                            .unwrap();
                        key(&mut app, KeyCode::F(6));
                        terminal.draw(|f| draw(f, &mut app)).unwrap();
                        assert!(buffer_text(&terminal).contains("secret-marker"));
                    }
                    key(&mut app, KeyCode::Esc);
                    key(&mut app, KeyCode::Char(','));
                    terminal.draw(|f| draw(f, &mut app)).unwrap();
                    let Some(Modal::Form(f)) = &mut app.modal else {
                        unreachable!()
                    };
                    f.focus = f
                        .fields
                        .iter()
                        .position(|f| f.path[0] == "language")
                        .unwrap();
                    key(&mut app, KeyCode::Enter);
                    terminal.draw(|f| draw(f, &mut app)).unwrap();
                    assert!(matches!(app.modal, Some(Modal::FieldChoice { .. })));
                    if width >= 60 {
                        click(&mut app, |t| matches!(t, Target::ChoiceRow(_)));
                        assert!(matches!(app.modal, Some(Modal::Form(_))));
                    }
                }
            }
        }
    }
}

use crate::app::{App, Modal};
use crate::mouse::{Hit, Pane, Target};
use bmcforge_core::ControlState;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState, Wrap},
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
    let light = app.config.settings.theme == "light";
    let bg = if light {
        Color::Rgb(240, 244, 250)
    } else {
        Color::Rgb(13, 19, 29)
    };
    let fg = if light {
        Color::Rgb(25, 36, 52)
    } else {
        Color::Rgb(210, 221, 235)
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(bg).fg(fg)),
        frame.area(),
    );
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(6),
        ])
        .split(frame.area());
    let title = format!(
        " BMCForge {} {} ",
        env!("CARGO_PKG_VERSION"),
        if app.demo { "[DEMO]" } else { "" }
    );
    let nav = Block::bordered().title(title);
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
        let width = (label.width() as u16 + 1).min(left.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let r = Rect::new(x, left.y, width, 1);
        frame.render_widget(
            Paragraph::new(label).style(
                (if page == app.page {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
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
    let footer = Block::default().borders(Borders::TOP);
    let footer_inner = footer.inner(layout[2]);
    frame.render_widget(footer, layout[2]);
    let footer_lines =
        Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(footer_inner);
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
    button_bar(frame, footer_lines[0], app, buttons);
    frame.render_widget(
        Paragraph::new(if app.identifying {
            format!(
                "{} {} {}/{}",
                spinner(),
                app.text("identifying"),
                app.pending.len(),
                app.identify_total
            )
        } else {
            app.locales
                .render(&app.config.settings.language, &app.status)
        })
        .wrap(Wrap { trim: false }),
        footer_lines[1],
    );
    let navigation = format!(
        "F8: {} · ↑↓ j/k · {}: {} · Enter: {} · Tab/Shift+Tab: 1–5",
        app.text("form-actions"),
        crate::interaction::key_label(
            app.target_key(&Target::Shortcut("select".into()))
                .unwrap_or(KeyCode::Char(' '))
        ),
        app.text("select"),
        app.text("confirm")
    );
    if footer_lines[1].height > 1 {
        frame.render_widget(
            Paragraph::new(navigation),
            Rect::new(
                footer_lines[1].x,
                footer_lines[1].bottom() - 1,
                footer_lines[1].width,
                1,
            ),
        );
    }
    if app.scanning && !app.identifying && footer_lines[1].height > 1 {
        frame.render_widget(
            Paragraph::new(format!(
                "{} {} {}/{} · F8: {}",
                spinner(),
                app.text("scanning"),
                app.discovery.len(),
                app.scan_total,
                app.text("form-actions")
            )),
            Rect::new(
                footer_lines[1].x,
                footer_lines[1].bottom() - 1,
                footer_lines[1].width,
                1,
            ),
        );
    }
    if app.modal.is_some() {
        app.hits.clear(); // A modal owns all input, including clicks outside its content.
        modal(frame, app);
    }
    // Apply feedback after rendering so tables and nested checkbox targets share the same treatment.
    for hit in &app.hits {
        if !matches!(hit.target, Target::Scroll(_) | Target::Editor { .. }) {
            let style = app.interaction.style(&hit.target);
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
fn header(cells: &[&str]) -> Row<'static> {
    Row::new(cells.iter().map(|s| Cell::from(s.to_string()))).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}
fn fleet(frame: &mut Frame, area: Rect, app: &mut App) {
    let parts = Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
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
            Row::new(vec![
                if app.selected.contains(&s.id) {
                    "[x]".into()
                } else {
                    "[ ]".into()
                },
                s.tags.join(","),
                s.name.clone(),
                s.endpoint(),
                state,
                rpm,
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Percentage(15),
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Min(12),
            Constraint::Length(8),
        ],
    )
    .header(header(&[
        "",
        app.text("group"),
        app.text("server"),
        app.text("bmc-endpoint"),
        app.text("status"),
        app.text("max-rpm"),
    ]))
    .block(Block::bordered().title(format!(
        " {} — {}: {} — {}: {} ",
        app.text("fleet"),
        app.text("selected"),
        app.selected.len(),
        app.text("filter"),
        app.search
    )))
    .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White));
    let mut state = TableState::default().with_selected(if visible.is_empty() {
        None
    } else {
        Some(app.cursor.min(visible.len() - 1))
    });
    frame.render_stateful_widget(table, parts[0], &mut state);
    table_hits(
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
    frame.render_widget(
        Paragraph::new(server_detail(app))
            .block(Block::bordered().title(app.text("server-details")))
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        parts[1],
    );
}
fn server_detail(app: &App) -> String {
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
            ControlState::Automatic => app.text("automatic").into(),
            ControlState::Requested { percent, zone } => {
                format!("{}: {percent}% / {zone}", app.text("manual"))
            }
            ControlState::RawRequested { value, zone } => {
                format!("{}: {value:#04x} / {zone}", app.text("raw"))
            }
            ControlState::Uncertain => app.text("uncertain").into(),
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
    frame.render_widget(
        Paragraph::new(server_detail(app))
            .block(Block::bordered().title(app.text("fan-control-identity")))
            .wrap(Wrap { trim: false }),
        parts[0],
    );
    let snapshot = app.current().and_then(|s| app.snapshots.get(&s.id));
    let filter = app.config.settings.sensor_filter.to_lowercase();
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
            Row::new(vec![
                s.id.clone(),
                s.name.clone(),
                s.kind.clone(),
                value,
                unit.into(),
                s.state.clone(),
                s.raw.iter().map(|b| format!("{b:02x} ")).collect(),
            ])
        })
        .collect();
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Min(12),
                Constraint::Length(14),
            ],
        )
        .header(header(&[
            "ID",
            app.text("sensor"),
            app.text("type"),
            app.text("reading"),
            app.text("unit"),
            app.text("state"),
            app.text("raw"),
        ]))
        .block(
            Block::bordered()
                .title(app.text("sensors-up-down-switch-server-pgup-pgdn-scroll-details")),
        ),
        parts[1],
    );
    let data: Vec<u64> = app
        .current()
        .and_then(|s| app.history.get(&s.id))
        .map(|v| v.iter().copied().collect())
        .unwrap_or_default();
    frame.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::default().fg(Color::Cyan))
            .block(Block::bordered().title(app.text("max-rpm"))),
        parts[2],
    );
}
fn discovery(frame: &mut Frame, area: Rect, app: &mut App) {
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
        .header(header(&[
            "",
            "BMC IPv4",
            app.text("probe-status"),
            app.text("note"),
        ]))
        .block(Block::bordered().title(format!(
            " {}/{} {} | {}: {} | i: {} ",
            app.discovery.len(),
            app.scan_total,
            app.text(if app.scanning { "scanning" } else { "idle" }),
            app.text("selected"),
            app.discovery_selected.len(),
            app.text("identify")
        )))
        .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White)),
        area,
        &mut state,
    );
    table_hits(
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
    let parts = if area.width >= 95 {
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
            .header(header(&[app.text("profile"), app.text("verification")]))
            .block(Block::bordered().title(app.text("profiles")))
            .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White)),
        parts[0],
        &mut state,
    );
    table_hits(
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
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(app.text("profiles")))
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        parts[1],
    );
}
fn logs(frame: &mut Frame, area: Rect, app: &mut App) {
    app.hits.push(Hit::new(area, Target::Scroll(Pane::Logs)));
    frame.render_widget(
        Paragraph::new(
            app.logs
                .iter()
                .map(|s| Line::from(app.locales.render(&app.config.settings.language, s)))
                .collect::<Vec<_>>(),
        )
        .block(Block::bordered().title(app.text("activity-redacted-x-exports-diagnostics")))
        .wrap(Wrap { trim: false })
        .scroll((app.log_scroll, 0)),
        area,
    );
}
fn button_bar(frame: &mut Frame, area: Rect, app: &mut App, buttons: Vec<(String, Target)>) {
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
        let style = if app.interaction.action_focus == Some(index) {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(label).style(style), rect);
        app.hits.push(Hit::new(rect, target));
        x = x.saturating_add(width + 1);
    }
}

fn table_hits(app: &mut App, area: Rect, offset: usize, items: Vec<Target>, pane: Pane) {
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
}
fn draw_form(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::form::FieldType;
    let Some(Modal::Form(form)) = &app.modal else {
        return;
    };
    let title = form.title.clone();
    let block = Block::bordered()
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
            Style::default().fg(Color::Red)
        } else if index == form.focus {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let block = Block::bordered()
            .title(format!(
                "{} / {} · {}/{}",
                app.text(field.group),
                label,
                index + 1,
                form.fields.len()
            ))
            .title_bottom(hint)
            .border_style(style);
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
    let error = if form.error.is_empty() {
        &form.fields[form.focus].error
    } else {
        &form.error
    };
    frame.render_widget(
        Paragraph::new(app.text(error).to_owned())
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: false }),
        parts[2],
    );
}
fn modal(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let popup = Rect {
        x: area.x + area.width / 20,
        y: area.y + area.height / 12,
        width: area.width.saturating_sub(area.width / 10),
        height: area.height.saturating_sub(area.height / 6),
    };
    frame.render_widget(Clear, popup);
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
            .block(Block::bordered().title(app.text(&field.label))),
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
                    .fg(Color::Cyan)
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
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    if matches!(app.modal, Some(Modal::Form(_))) {
        draw_form(frame, popup, app);
        button_bar(frame, buttons_area, app, buttons);
        return;
    }
    let Some(modal) = &mut app.modal else {
        return;
    };
    match modal {
        Modal::Form(_) | Modal::FieldChoice { .. } => unreachable!(),
        Modal::Languages { cursor } => {
            let mut state = TableState::default().with_selected(Some(*cursor));
            frame.render_stateful_widget(
                Table::new(
                    languages.iter().map(|s| Row::new([s.clone()])),
                    [Constraint::Min(1)],
                )
                .block(Block::bordered().title("🌐 ↑↓ Enter · F8"))
                .row_highlight_style(Style::default().fg(Color::Cyan)),
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
        }
        Modal::ModelChoice { query, cursor } => {
            let parts = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).split(popup);
            frame.render_widget(
                Paragraph::new(format!(
                    "{evidence}\n{}: {query} | F5: {} | F6: {}",
                    labels["search"], labels["unknown"], labels["skip"]
                ))
                .block(Block::bordered().title(model_title))
                .wrap(Wrap { trim: false }),
                parts[0],
            );
            let mut state = TableState::default().with_selected(Some(*cursor));
            frame.render_stateful_widget(
                Table::new(
                    model_rows.iter().map(|s| Row::new([s.clone()])),
                    [Constraint::Min(1)],
                )
                .row_highlight_style(Style::default().fg(Color::Cyan)),
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
        }
        Modal::Import {
            servers,
            authenticate,
            duplicates,
        } => frame.render_widget(
            Paragraph::new(format!(
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
            ))
            .block(Block::bordered().title(labels["preview"].clone()))
            .wrap(Wrap { trim: false })
            .scroll((app.modal_scroll, 0)),
            popup,
        ),
        Modal::Editor {
            title,
            editor,
            error,
            ..
        } => {
            let block = Block::bordered().title(title.clone()).title_bottom(format!(
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
                    .style(Style::default().fg(Color::Red))
                    .wrap(Wrap { trim: false }),
                parts[1],
            );
            if parts[0].height > 0 && parts[0].width > 0 {
                frame.set_cursor_position((
                    parts[0].x + (width - horizontal) as u16,
                    parts[0].y + (editor.row - editor.scroll) as u16,
                ));
            }
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
                hosts
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                errors.join("\n"),
                labels["continue"],
                labels["cancel"]
            );
            frame.render_widget(
                Paragraph::new(text)
                    .block(Block::bordered().title(labels["preview"].clone()))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Modal::Confirm { title, text, .. } => {
            let mut lines = vec![
                Line::styled(
                    title.clone(),
                    Style::default()
                        .fg(Color::Yellow)
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
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(labels["preview"].clone()))
                    .wrap(Wrap { trim: false })
                    .scroll((app.modal_scroll, 0)),
                popup,
            );
        }
        Modal::Info(text) => frame.render_widget(
            Paragraph::new(text.clone())
                .block(Block::bordered().title(" BMCForge "))
                .wrap(Wrap { trim: false })
                .scroll((app.modal_scroll, 0)),
            popup,
        ),
        Modal::Remove { name, .. } => frame.render_widget(
            Paragraph::new(format!(
                "{}\n{name}\n{}",
                labels["remove-server"], labels["safety"]
            ))
            .block(Block::bordered().title(labels["confirm"].clone()))
            .wrap(Wrap { trim: false }),
            popup,
        ),
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
            Color::Cyan
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
            Color::Yellow
        );
        app.interaction.pressed =
            Some((Target::Page(4), Instant::now() - Duration::from_millis(151)));
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_ne!(
            terminal.backend().buffer()[(hit.area.x, hit.area.y)].bg,
            Color::Yellow
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

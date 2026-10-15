use crate::app::{App, Modal};
use bmcforge_core::ControlState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState, Tabs, Wrap,
    },
};
use unicode_width::UnicodeWidthStr;

pub fn draw(frame: &mut Frame, app: &mut App) {
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
            Constraint::Length(4),
        ])
        .split(frame.area());
    let titles = if app.config.settings.language == "en" {
        vec![
            "1 Fleet",
            "2 Sensors",
            "3 Discovery",
            "4 Profiles",
            "5 Activity",
        ]
    } else {
        vec![
            "1 服务器",
            "2 传感器",
            "3 网络发现",
            "4 适配库",
            "5 操作日志",
        ]
    };
    let title = format!(
        " BMCForge {} {} ",
        env!("CARGO_PKG_VERSION"),
        if app.demo { "[DEMO]" } else { "" }
    );
    frame.render_widget(
        Tabs::new(titles)
            .select(app.page)
            .block(Block::bordered().title(title))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        layout[0],
    );
    match app.page {
        0 => fleet(frame, layout[1], app),
        1 => sensors(frame, layout[1], app),
        2 => discovery(frame, layout[1], app),
        3 => profiles(frame, layout[1], app),
        _ => logs(frame, layout[1], app),
    }
    let shortcut = [
        "import",
        "discover",
        "edit",
        "manual",
        "automatic",
        "select",
        "search",
        "settings",
        "quit",
    ]
    .map(|name| {
        format!(
            "{}:{}",
            app.config
                .settings
                .keys
                .get(name)
                .map(|s| if s == " " { "Space" } else { s })
                .unwrap_or("?"),
            name
        )
    })
    .join("  ");
    let footer = Block::default().borders(Borders::TOP);
    let footer_inner = footer.inner(layout[2]);
    frame.render_widget(footer, layout[2]);
    let footer_lines =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(footer_inner);
    frame.render_widget(
        Paragraph::new(format!("{shortcut}  F1:help")),
        footer_lines[0],
    );
    frame.render_widget(
        Paragraph::new(app.status.clone()).wrap(Wrap { trim: false }),
        footer_lines[1],
    );
    if app.modal.is_some() {
        modal(frame, app);
    }
}
fn header(cells: &[&str]) -> Row<'static> {
    Row::new(cells.iter().map(|s| Cell::from(s.to_string()))).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}
fn fleet(frame: &mut Frame, area: Rect, app: &App) {
    let parts = Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).split(area);
    let visible = app.visible();
    let rows: Vec<_> = visible
        .iter()
        .map(|i| {
            let s = &app.config.servers[*i];
            let snapshot = app.snapshots.get(&s.id);
            let state = snapshot.map(|s| s.status.as_str()).unwrap_or("Connecting");
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
                state.into(),
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
        app.tr("分组", "Group"),
        app.tr("服务器", "Server"),
        app.tr("BMC 地址", "BMC endpoint"),
        app.tr("状态", "Status"),
        "Max RPM",
    ]))
    .block(Block::bordered().title(format!(
        " {} — {} selected — filter: {} ",
        app.tr("服务器列表", "Fleet"),
        app.selected.len(),
        app.search
    )))
    .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White));
    let mut state = TableState::default().with_selected(if visible.is_empty() {
        None
    } else {
        Some(app.cursor.min(visible.len() - 1))
    });
    frame.render_stateful_widget(table, parts[0], &mut state);
    frame.render_widget(
        Paragraph::new(server_detail(app))
            .block(Block::bordered().title(app.tr("服务器详情", "Server details")))
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        parts[1],
    );
}
fn server_detail(app: &App) -> String {
    let Some(server) = app.current() else {
        return app.tr("按 i 批量添加；按 d 扫描网段。支持每台服务器独立账号密码。","Press i to import servers or d to discover a subnet. Each server has independent credentials.").into();
    };
    let snapshot = app.snapshots.get(&server.id);
    let identity = snapshot
        .map(|s| {
            format!(
                "{} | IANA {} | product 0x{:04x} | firmware {}",
                s.identity.model,
                s.identity.manufacturer_id,
                s.identity.product_id,
                s.identity.firmware
            )
        })
        .unwrap_or_else(|| "Identifying…".into());
    let control = snapshot
        .map(|s| match s.control {
            ControlState::Unknown => "Unknown".into(),
            ControlState::Automatic => "BMC automatic requested".into(),
            ControlState::Requested { percent, zone } => {
                format!("Manual requested: {percent}% / zone {zone}")
            }
            ControlState::Uncertain => "UNCERTAIN — restore automatic mode".into(),
        })
        .unwrap_or_else(|| "Unknown".into());
    let candidates = snapshot
        .map(|s| app.registry.candidates(&s.identity).join(", "))
        .unwrap_or_default();
    format!(
        "{identity}\nProfile: {} | Candidates: {}\nControl: {control} | Actual duty: unknown | m: manual / a: automatic\nBMC restore is attempted on exit. Kill, power loss or network loss cannot guarantee recovery.",
        server.profile.as_deref().unwrap_or("auto / generic"),
        if candidates.is_empty() {
            "none"
        } else {
            &candidates
        }
    )
}
fn sensors(frame: &mut Frame, area: Rect, app: &App) {
    let parts = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(5),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(server_detail(app))
            .block(Block::bordered().title(app.tr("风扇控制与设备身份", "Fan control & identity")))
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
            app.tr("传感器", "Sensor"),
            app.tr("类型", "Type"),
            app.tr("实测值", "Reading"),
            app.tr("单位", "Unit"),
            app.tr("状态", "State"),
            "Raw",
        ]))
        .block(Block::bordered().title(app.tr(
            "传感器：上下切换服务器，PgUp/PgDn 滚动详情",
            "Sensors: Up/Down switch server, PgUp/PgDn scroll details",
        ))),
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
            .block(Block::bordered().title(" Max fan RPM — recent samples ")),
        parts[2],
    );
}
fn discovery(frame: &mut Frame, area: Rect, app: &App) {
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
                format!("{:?}", r.status),
                if r.status == bmcforge_ipmi::discovery::ProbeStatus::IpmiCandidate {
                    "Unauthenticated candidate".into()
                } else {
                    "Unknown / not proof of absence".into()
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
            app.tr("探测状态", "Probe status"),
            app.tr("说明", "Note"),
        ]))
        .block(Block::bordered().title(format!(
            " {}/{} {} | d: scan/cancel  f: candidates/all  Space: select  i: authenticate & add ",
            app.discovery.len(),
            app.scan_total,
            if app.scanning { "scanning" } else { "idle" }
        )))
        .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White)),
        area,
        &mut state,
    );
}
fn profiles(frame: &mut Frame, area: Rect, app: &App) {
    let parts = if area.width >= 95 {
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area)
    };
    let rows: Vec<_> = app
        .registry
        .profiles
        .values()
        .map(|p| Row::new(vec![p.id.clone(), format!("{:?}", p.verification)]))
        .collect();
    let mut state = TableState::default().with_selected(Some(app.profile_cursor));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Percentage(65), Constraint::Min(10)])
            .header(header(&["Profile", "Verification"]))
            .block(Block::bordered().title(" Enter: select profile | u: default credentials "))
            .row_highlight_style(Style::default().bg(Color::Rgb(35, 58, 78)).fg(Color::White)),
        parts[0],
        &mut state,
    );
    let text = app.registry.profiles.values().nth(app.profile_cursor).map(|p|format!("{}\n{}\n\n{}\n\nZones: {}\n\nSources:\n{}\n\nFactory credentials (never automatically attempted):\n{}",p.vendor,p.description,p.notes,p.fan.as_ref().map(|f|f.zones.iter().map(|z|format!("{}={}",z.id,z.name)).collect::<Vec<_>>().join(", ")).unwrap_or_else(||"Monitoring only".into()),p.sources.join("\n"),p.default_credentials.iter().map(|c|format!("{} / {} — {}\n{}",c.username,c.password.as_deref().unwrap_or("device-specific"),c.note,c.source)).collect::<Vec<_>>().join("\n"))).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(" Profile details "))
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        parts[1],
    );
}
fn logs(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(
            app.logs
                .iter()
                .map(|s| Line::from(s.clone()))
                .collect::<Vec<_>>(),
        )
        .block(Block::bordered().title(app.tr(
            "操作日志（已脱敏）；x 导出诊断",
            "Activity (redacted); x exports diagnostics",
        )))
        .wrap(Wrap { trim: false })
        .scroll((app.log_scroll, 0)),
        area,
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
    let Some(modal) = &mut app.modal else {
        return;
    };
    match modal {
        Modal::Import { servers, authenticate, duplicates } => frame.render_widget(Paragraph::new(format!("{} servers | {} existing endpoints skipped\n{}\n\n{}\nEnter: confirm | Esc: cancel",servers.len(),duplicates,servers.iter().take(14).map(|s|format!("{} | user {} | profile {}",s.endpoint(),s.credentials.username,s.profile.as_deref().unwrap_or("auto"))).collect::<Vec<_>>().join("\n"),if *authenticate { "Authenticate with exactly the selected credentials, then save successful devices" } else { "Save these servers; credentials will be plaintext" })).block(Block::bordered().title(" Final import preview ")).wrap(Wrap { trim:false }),popup),
        Modal::Editor {
            title,
            editor,
            error,
            ..
        } => {
            let block = Block::bordered().title(title.clone()).title_bottom(
                " F2 / Ctrl+Enter: save | Esc: cancel | plaintext credentials remain local ",
            );
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
                "{} targets | {} duplicates skipped | {} invalid lines{}\n\n{}\n{}\n\nEnter: confirm this target count and continue | Esc: cancel",
                hosts.len(),
                duplicates,
                errors.len(),
                if large {
                    " — LARGE SCAN: review address count"
                } else {
                    ""
                },
                hosts
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                errors.join("\n")
            );
            frame.render_widget(
                Paragraph::new(text)
                    .block(Block::bordered().title(" Import / discovery preview "))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Modal::Confirm { title, text, .. } => {
            let mut lines=vec![Line::styled(title.clone(),Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),Line::from("")];
            lines.extend(text.lines().map(|line|Line::from(line.to_owned())));
            lines.push(Line::from("")); lines.push(Line::from("Enter: apply to all listed servers | Esc: cancel | PgUp/PgDn: review"));
            frame.render_widget(
                Paragraph::new(lines)
                .block(Block::bordered().title(" Review control operation "))
                .wrap(Wrap { trim: false }).scroll((app.modal_scroll,0)),
                popup,
            );
        }
        Modal::Info(text) => frame.render_widget(
            Paragraph::new(format!("{text}\nEnter / Esc: close"))
            .block(Block::bordered().title(" BMCForge "))
            .wrap(Wrap { trim: false }).scroll((app.modal_scroll,0)),
            popup,
        ),
        Modal::Remove { name,.. } => frame.render_widget(Paragraph::new(format!("Remove {name}?\nBMC automatic restore will be attempted first.\nEnter: remove | Esc: cancel")).block(Block::bordered().title(" Remove server ")).wrap(Wrap { trim:false }),popup),
    }
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
        for language in ["en", "zh-CN"] {
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
}

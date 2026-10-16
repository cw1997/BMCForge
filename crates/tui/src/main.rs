mod app;
mod chrome;
mod commands;
mod editor;
mod form;
mod i18n;
mod interaction;
mod local_profiles;
mod mouse;
mod runtime;
mod theme;
mod ui;

use anyhow::{Context, Result, bail};
use app::{Action, App};
use bmcforge_core::config::{Config, default_path};
use bmcforge_profiles::Registry;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyModifiers,
    },
    execute,
};
use futures_util::StreamExt;
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "BMCForge — native IPMI fleet sensors and fan control TUI"
)]
struct Args {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    profiles_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    locales_dir: Option<PathBuf>,
    #[arg(long)]
    demo: bool,
    #[command(subcommand)]
    command: Option<CliCommand>,
}
#[derive(Subcommand)]
enum CliCommand {
    /// Validate configuration without connecting to servers.
    ValidateConfig,
    /// Validate every built-in/external profile, including command examples.
    ValidateProfiles,
    ValidateLocales,
    ValidateModels,
    /// List sourced model IDs and their associated adapters; vendor accepts native aliases.
    ListModels {
        #[arg(long)]
        vendor: Option<String>,
        #[arg(long)]
        json: bool,
    },
    ModelSchema,
    LocaleSchema,
    /// Print the profile JSON schema (applies to parsed TOML data).
    ProfileSchema,
    /// Create a new configuration; never overwrites an existing file.
    Init,
    /// Exercise simulated identity, sensors, control, discovery and restore without a terminal.
    DemoCheck,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let path = match args.config {
        Some(path) => path,
        None => default_path()?,
    };
    let mut registry = Registry::load(args.profiles_dir.as_deref())?;
    if !args.demo {
        local_profiles::recover(&path)?;
        local_profiles::load(&path, &mut registry)?;
    }
    let locales = i18n::Locales::load(args.locales_dir.as_deref())?;
    let catalog = bmcforge_profiles::models::Catalog::load(args.profiles_dir.as_deref())?;
    for warning in &locales.diagnostics {
        eprintln!("warning: locale ignored: {warning}");
    }
    match args.command {
        Some(CliCommand::ListModels { vendor, json }) => {
            let query = vendor.unwrap_or_default().to_lowercase();
            let rows: Vec<_> = catalog
                .models
                .values()
                .filter(|m| {
                    format!("{} {} {}", m.vendor, m.name, m.aliases.join(" "))
                        .to_lowercase()
                        .contains(&query)
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for m in rows {
                    println!(
                        "{} | {} | {} | {}",
                        m.id,
                        m.vendor,
                        m.name,
                        m.profile.as_deref().unwrap_or("generic")
                    );
                }
            }
            return Ok(());
        }
        Some(CliCommand::ValidateLocales) => {
            anyhow::ensure!(
                locales.diagnostics.is_empty(),
                "external locale validation failed"
            );
            println!("{} language packs valid", locales.packs.len());
            return Ok(());
        }
        Some(CliCommand::ValidateModels) => {
            for m in catalog.models.values() {
                if let Some(p) = &m.profile {
                    anyhow::ensure!(
                        registry.profiles.contains_key(p),
                        "unknown model profile {p}"
                    );
                }
            }
            println!("{} models valid", catalog.models.len());
            return Ok(());
        }
        Some(CliCommand::ModelSchema) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&bmcforge_profiles::models::Catalog::schema())?
            );
            return Ok(());
        }
        Some(CliCommand::LocaleSchema) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&i18n::Locales::schema())?
            );
            return Ok(());
        }
        Some(CliCommand::ValidateConfig) => {
            let config = Config::load(&path)?;
            validate_profile_references(&config, &registry)?;
            println!("Configuration valid: {}", path.display());
            return Ok(());
        }
        Some(CliCommand::ValidateProfiles) => {
            for p in registry.profiles.values() {
                println!("{}: {:?}", p.id, p.verification);
            }
            return Ok(());
        }
        Some(CliCommand::ProfileSchema) => {
            println!("{}", serde_json::to_string_pretty(&Registry::schema())?);
            return Ok(());
        }
        Some(CliCommand::Init) => {
            if path.exists() {
                bail!("configuration already exists: {}", path.display());
            }
            Config::default().save(&path)?;
            println!("Created {} (plaintext credentials)", path.display());
            return Ok(());
        }
        Some(CliCommand::DemoCheck) => return demo_check(registry).await,
        None => {}
    }
    let mut config = if args.demo {
        Config::default()
    } else if path.exists() {
        Config::load(&path)?
    } else {
        Config::default()
    };
    if args.demo {
        config.servers = bmcforge_ipmi::mock::servers();
    }
    validate_profile_references(&config, &registry)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("TUI requires an interactive terminal; use demo-check for headless smoke testing");
    }
    let first_open = mark_help_seen(&mut config, &path, args.demo)?;
    let mut app = App::new(config, path, registry, args.demo);
    if first_open {
        app.page = 4;
    }
    app.locales = locales;
    app.catalog = catalog;
    app.log(i18n::Message::new("safety"));
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut runtime = runtime::Runtime::new(tx, &app.config.settings, args.demo);
    for server in app.config.servers.clone() {
        runtime.add(server);
    }
    let mut terminal = ratatui::init();
    let result = async {
        execute!(io::stdout(), EnableBracketedPaste)?;
        if app.config.settings.mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        event_loop(&mut terminal, &mut app, &mut runtime, &mut rx).await
    }
    .await;
    terminal
        .draw(|f| {
            f.render_widget(
                ratatui::widgets::Paragraph::new(app.text("automatic")),
                f.area(),
            );
        })
        .ok();
    // Keep the receiver draining while workers restore, so a full event channel cannot deadlock exit.
    let failures = runtime.shutdown(&mut rx).await;
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    for failure in &failures {
        eprintln!("{}", bmcforge_core::redact(failure, &app.config.secrets()));
    }
    result?;
    if !failures.is_empty() {
        bail!(
            "{} server(s) could not be restored; check BMC fan mode",
            failures.len()
        );
    }
    Ok(())
}

fn mark_help_seen(config: &mut Config, path: &std::path::Path, demo: bool) -> Result<bool> {
    if config.help_seen {
        return Ok(false);
    }
    config.help_seen = true;
    if !demo {
        config.save(path)?;
    }
    Ok(true)
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn first_open_help_is_saved_once_and_demo_does_not_write() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let mut config = Config::default();

        assert!(mark_help_seen(&mut config, &path, false)?);
        assert!(Config::load(&path)?.help_seen);
        assert!(!mark_help_seen(&mut config, &path, false)?);

        let demo_path = dir.path().join("demo.toml");
        let mut demo_config = Config::default();
        assert!(mark_help_seen(&mut demo_config, &demo_path, true)?);
        assert!(!demo_path.exists());
        Ok(())
    }
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    runtime: &mut runtime::Runtime,
    rx: &mut tokio::sync::mpsc::Receiver<runtime::Message>,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let stop = termination();
    tokio::pin!(stop);
    let mut mouse_enabled = app.config.settings.mouse;
    loop {
        if mouse_enabled != app.config.settings.mouse {
            mouse_enabled = app.config.settings.mouse;
            if mouse_enabled {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
        }
        let actions = tokio::select! {
            _=&mut stop => return Ok(()),
            _=tick.tick() => { app.resume_review(); terminal.draw(|f|ui::draw(f,app))?; vec![] },
            Some(message)=rx.recv() => app.message(message),
            event=events.next() => match event {
                Some(Ok(Event::Key(key))) => if key.code==KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) { return Ok(()); } else { app.input(key) },
                Some(Ok(Event::Paste(text))) => { app.paste(&text); vec![] },
                Some(Ok(Event::Mouse(mouse))) => {
                    app.mouse(mouse)
                },
                Some(Ok(_))=>vec![], Some(Err(error))=>return Err(error.into()),None=>return Ok(()),
            }
        };
        for action in actions {
            match action {
                Action::Quit => return Ok(()),
                Action::Add(servers) => {
                    for server in servers {
                        runtime.add(server);
                    }
                }
                Action::Forget(id) => runtime.forget(&id),
                Action::Configure(server) => {
                    if let Err(e) =
                        runtime.send(&server.id.clone(), runtime::Command::Configure(server))
                    {
                        app.log(format!("{e:#}"));
                    }
                }
                Action::Send(id, command) => {
                    if let Err(e) = runtime.send(&id, command) {
                        app.log(format!("{e:#}"));
                    }
                }
                Action::Scan(hosts, port) => {
                    app.scan_generation = runtime.scan(hosts, port, app.config.settings.clone());
                }
                Action::CancelScan => runtime.scan_cancel.cancel(),
                Action::Authenticate(servers) => {
                    app.identify_generation = runtime.authenticate(servers);
                }
                Action::CancelIdentify => {
                    app.identify_generation = runtime.cancel_identify();
                }
            }
        }
    }
}

fn validate_profile_references(config: &Config, registry: &Registry) -> Result<()> {
    for server in &config.servers {
        if let Some(profile) = &server.profile {
            anyhow::ensure!(
                registry.profiles.contains_key(profile),
                "server {} references unknown profile {}",
                server.id,
                profile
            );
        }
    }
    Ok(())
}
async fn termination() {
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_logoff, ctrl_shutdown};
        if let (Ok(mut close), Ok(mut shutdown), Ok(mut logoff), Ok(mut interrupt)) =
            (ctrl_close(), ctrl_shutdown(), ctrl_logoff(), ctrl_break())
        {
            tokio::select! { _=tokio::signal::ctrl_c()=>{},_=close.recv()=>{},_=shutdown.recv()=>{},_=logoff.recv()=>{},_=interrupt.recv()=>{} }
            return;
        }
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! { _=tokio::signal::ctrl_c()=>{},_=term.recv()=>{} };
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
async fn demo_check(registry: Registry) -> Result<()> {
    let mut simulated = bmcforge_ipmi::mock::servers();
    simulated.push(bmcforge_core::config::Server {
        id: "demo-uis".into(),
        host: "192.0.2.90".into(),
        name: "H3C UIS 2000 G6".into(),
        profile: Some("h3c-uis2000-g6".into()),
        allow_experimental: true,
        ..Default::default()
    });
    for server in simulated {
        let profile = registry
            .profiles
            .get(server.profile.as_deref().context("profile")?)
            .context("profile missing")?;
        let mut device = bmcforge_ipmi::Device::new(server, true);
        device.poll().await?;
        let Some(fan) = profile.fan.as_ref() else {
            device.shutdown().await?;
            continue;
        };
        for zone in &fan.zones {
            let value = fan
                .raw_values
                .first()
                .copied()
                .map(bmcforge_core::ControlValue::Raw)
                .unwrap_or(bmcforge_core::ControlValue::Percent(40));
            device.manual_value(profile, value, zone.id).await?;
        }
        device.poll().await?;
        println!(
            "{}: {} sensors; typed manual request {:?}; actual duty unknown",
            device.identity.model,
            device.sensors.len(),
            device.control
        );
        device.shutdown().await?;
        anyhow::ensure!(!device.taken_over, "restore failed");
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    bmcforge_ipmi::discovery::scan(
        vec!["192.0.2.10".into(), "192.0.2.11".into()],
        623,
        Default::default(),
        tokio_util::sync::CancellationToken::new(),
        tx,
        true,
    )
    .await?;
    let mut count = 0;
    while rx.recv().await.is_some() {
        count += 1;
    }
    anyhow::ensure!(count == 2, "demo discovery failed");
    let mut review = App::new(Config::default(), "demo-unused.toml".into(), registry, true);
    review.identifying = true;
    review.identify_generation = 1;
    review.identify_total = 4;
    for last in 20..=23 {
        let server = bmcforge_core::config::Server {
            id: format!("identified-{last}"),
            host: format!("192.0.2.{last}"),
            name: format!("Demo {last}"),
            credentials: bmcforge_core::config::Credentials {
                username: "demo".into(),
                password: "demo".into(),
            },
            allow_experimental: true,
            ..Default::default()
        };
        let mut device = bmcforge_ipmi::Device::new(server.clone(), true);
        let identity = device.identify().await?;
        device.shutdown().await?;
        review.message(runtime::Message::Identified(1, server, identity, None));
    }
    review.message(runtime::Message::IdentificationFinished(1));
    while let Some(app::Modal::ModelChoice { query, .. }) = &mut review.modal {
        *query = "3650".into();
        let key = if review.model_options("3650").is_empty() {
            KeyCode::F(5)
        } else {
            KeyCode::Enter
        };
        review.input(crossterm::event::KeyEvent::new(key, KeyModifiers::NONE));
    }
    anyhow::ensure!(
        review.config.servers.is_empty(),
        "identification saved before confirmation"
    );
    let actions = review.input(crossterm::event::KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ));
    anyhow::ensure!(
        matches!(actions.as_slice(),[Action::Add(servers)] if servers.len()==4),
        "identity review failed"
    );
    anyhow::ensure!(
        review
            .config
            .servers
            .iter()
            .filter(|s| s.model_id.is_some())
            .count()
            == 3,
        "unique/ambiguous recognition failed"
    );
    println!(
        "Identity-only batches, unique/ambiguous model review and final confirmation passed (4 simulated devices)."
    );
    println!(
        "Demo discovery and BMC automatic restore passed. Simulation is not hardware verification."
    );
    Ok(())
}

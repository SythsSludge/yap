use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture,
};
use ratatui::crossterm::execute;
use ratatui_image::picker::Picker;
use std::path::PathBuf;
use std::time::Duration;
use yap::app::App;
use yap::config::{self, Config, Paths};
use yap::drawer::Drawer;
use yap::net::{self, NetCommand, NetConfig, NetEvent};
use yap::traffic::TrafficLog;

/// A terminal client for YiffSpot.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Server to connect to for this session (wss://…); not saved.
    #[arg(long, value_name = "URL")]
    server: Option<String>,
    /// Preference profile to activate.
    #[arg(long, short)]
    profile: Option<String>,
    /// Theme for this session; not saved.
    #[arg(long)]
    theme: Option<String>,
    /// Use this config file instead of the default.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Also append every websocket frame to this file as JSON Lines.
    #[arg(long, value_name = "PATH")]
    traffic_log: Option<PathBuf>,
    /// Don't capture the mouse, so the terminal's own text selection works.
    #[arg(long)]
    no_mouse: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect, print the raw traffic for a few seconds, and disconnect.
    /// Never searches for a partner, so it's safe against the live site.
    Probe {
        #[arg(default_value_t = 5)]
        seconds: u64,
    },
    /// Export profiles (all of them plus settings, or just one) to a .toml or .json file.
    Export {
        path: PathBuf,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Import profiles from a yap export or a yiffspot.com localStorage dump.
    Import {
        path: PathBuf,
        /// Also replace your settings with the file's.
        #[arg(long)]
        with_settings: bool,
    },
    /// Save settings, profiles, themes, buddies, the drawer, stats and history to one file.
    Backup {
        path: PathBuf,
        /// Include chat logs too.
        #[arg(long)]
        with_logs: bool,
    },
    /// Put a backup back. Files it replaces are kept as `<name>.before-restore`.
    Restore { path: PathBuf },
    /// Run as an MCP server for an AI app (it starts this; you don't). Talks to the
    /// yap you have open, if Settings → AI tools is on.
    Mcp,
    /// Print shell completions (bash, zsh, fish, elvish, powershell) for packaging.
    #[command(hide = true)]
    Completions {
        shell: clap_complete::Shell,
        /// The command name to complete (`yiff` is the other name yap installs as).
        #[arg(long, default_value = "yap")]
        bin: String,
    },
    /// Print the man page (roff) for packaging.
    #[command(hide = true)]
    Manpage,
    /// List available themes.
    Themes,
    /// Show where yap keeps its files.
    Paths,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // These only describe the command line, so they must work without a config (e.g.
    // while a package is being built).
    match &cli.command {
        Some(Cmd::Completions { shell, bin }) => {
            clap_complete::generate(*shell, &mut <Cli as clap::CommandFactory>::command(), bin, &mut std::io::stdout());
            return Ok(());
        }
        Some(Cmd::Manpage) => {
            clap_mangen::Man::new(<Cli as clap::CommandFactory>::command()).render(&mut std::io::stdout())?;
            return Ok(());
        }
        _ => {}
    }
    let paths = Paths::discover(cli.config.clone())?;
    let first_run = !paths.config_file.exists();
    let (mut config, warnings) = Config::load(&paths.config_file)?;
    let runtime = tokio::runtime::Runtime::new()?;

    match cli.command {
        Some(Cmd::Probe { seconds }) => {
            let url = cli.server.unwrap_or(config.settings.server_url);
            return runtime.block_on(probe(&url, seconds));
        }
        Some(Cmd::Export { path, profile }) => {
            let doc = config.export(profile.as_deref())?;
            config::export_to_file(&doc, &path)?;
            println!("Exported to {}", path.display());
            return Ok(());
        }
        Some(Cmd::Import { path, with_settings }) => {
            let report = config.import(config::read_import(&path)?, with_settings);
            config.save(&paths.config_file)?;
            println!("Imported profiles: {}", report.profiles.join(", "));
            if report.settings_applied {
                println!("Settings replaced.");
            }
            for w in report.warnings {
                eprintln!("warning: {w}");
            }
            return Ok(());
        }
        Some(Cmd::Backup { path, with_logs }) => {
            let backup = yap::backup::Backup::create(&paths, with_logs)?;
            backup.write(&path)?;
            println!("Backed up {} files to {}", backup.files.len(), path.display());
            return Ok(());
        }
        Some(Cmd::Restore { path }) => {
            let written = yap::backup::Backup::read(&path)?.restore(&paths)?;
            for file in &written {
                println!("restored {}", file.display());
            }
            println!("{} files restored.", written.len());
            return Ok(());
        }
        Some(Cmd::Mcp) => {
            #[cfg(unix)]
            return yap::mcp::serve(&mut yap::mcp::SocketRelay { path: yap::mcp::socket_path(&paths) });
            #[cfg(not(unix))]
            bail!("yap mcp needs a Unix system for now");
        }
        Some(Cmd::Completions { .. } | Cmd::Manpage) => unreachable!("handled above"),
        Some(Cmd::Themes) => {
            let (themes, errors) = yap::theme::load_all(Some(&paths.themes_dir));
            for t in themes {
                let marker = if t.name == config.settings.theme { "*" } else { " " };
                println!("{marker} {}", t.name);
            }
            for e in errors {
                eprintln!("warning: {e}");
            }
            println!("\nCustom themes go in {}", paths.themes_dir.display());
            return Ok(());
        }
        Some(Cmd::Paths) => {
            println!("config  {}", paths.config_file.display());
            println!("themes  {}", paths.themes_dir.display());
            println!("drawer  {}", paths.drawer_file.display());
            println!("logs    {}", paths.logs_dir.display());
            println!("history {}", paths.history_file.display());
            println!("images  {}", paths.downloads_dir.display());
            return Ok(());
        }
        None => {}
    }

    if let Some(name) = &cli.profile {
        config.set_active(name).with_context(|| format!("--profile {name}"))?;
    }
    if let Some(server) = &cli.server {
        net::websocket_url(server).map_err(anyhow::Error::msg)?;
    }
    let drawer = Drawer::load(&paths.drawer_file)?;
    let (logs, log_warnings) = yap::logs::Logs::load_dir(&paths.logs_dir);
    let stats = yap::stats::Stats::load(&paths.stats_file);
    let (history, history_warnings) = yap::history::History::load(&paths.history_file);
    let (themes, theme_errors) = yap::theme::load_all(Some(&paths.themes_dir));
    if let Some(name) = &cli.theme
        && !themes.iter().any(|t| &t.name == name)
    {
        bail!("no theme called `{name}` (see `yap themes`)");
    }
    let traffic_sink = match &cli.traffic_log {
        Some(path) => Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening {}", path.display()))?,
        ),
        None => None,
    };

    let mut terminal = ratatui::init();
    install_panic_hook();
    let mouse = !cli.no_mouse;
    let setup = || -> std::io::Result<()> {
        let mut out = std::io::stdout();
        execute!(out, EnableBracketedPaste, EnableFocusChange)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        Ok(())
    };
    let result = setup().map_err(anyhow::Error::from).and_then(|()| {
        // Ask the terminal which graphics protocol it speaks (kitty, sixel, iTerm2)
        // before anything else starts reading stdin.
        let picker = if config.settings.images.enabled {
            yap::images::picker_without_query()
                .unwrap_or_else(|| Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()))
        } else {
            Picker::halfblocks()
        };
        let mut app = App::new(paths, config, drawer, themes, picker);
        app.logs = logs;
        app.history = history;
        match yap::activity::Activity::load(&app.paths.activity_file) {
            Ok(activity) => app.activity = activity,
            Err(e) => app.toast(yap::app::Level::Warning, format!("{e:#}; starting fresh")),
        }
        app.load_speller();
        for e in app.load_buddies() {
            app.toast(yap::app::Level::Warning, e);
        }
        if first_run {
            app.welcome();
        }
        if let Some(tabs) = yap::app::Tabs::load(&app.paths.tabs_file) {
            app.restore_tabs(tabs);
        }
        match stats {
            Ok(stats) => app.stats = stats,
            Err(e) => app.toast(yap::app::Level::Warning, format!("{e:#}; starting fresh stats")),
        }
        app.server_override = cli.server;
        if let Some(name) = &cli.theme {
            app.set_theme(name, false);
        }
        if let Some(file) = traffic_sink {
            app.traffic = TrafficLog::new(app.config.settings.traffic.capacity).with_sink(Box::new(file));
        }
        for w in warnings.into_iter().chain(theme_errors).chain(log_warnings).chain(history_warnings) {
            app.toast(yap::app::Level::Warning, w);
        }
        runtime.block_on(yap::runtime::run(&mut terminal, app, mouse))
    });
    restore_terminal();
    result
}

fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
    ratatui::restore();
}

/// ratatui's hook restores raw mode and the alternate screen; also undo the extra
/// modes we enabled so a crash doesn't leave the shell capturing mouse events.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
        previous(info);
    }));
}

async fn probe(url: &str, seconds: u64) -> Result<()> {
    let url = net::websocket_url(url).map_err(anyhow::Error::msg)?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = net::spawn(NetConfig::new(url), tx);
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut closing = false;
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(NetEvent::Traffic(t)) => {
                    println!("{} {} {:<5} {:>5}  {}", t.at.format("%H:%M:%S%.3f"), t.dir.arrow(), t.kind.label(), t.size, t.body);
                }
                Some(NetEvent::Closed { reason }) => {
                    println!("closed: {reason}");
                    break;
                }
                Some(_) => {}
                None => break,
            },
            _ = &mut deadline, if !closing => {
                closing = true;
                handle.send(NetCommand::Close);
            }
        }
    }
    Ok(())
}

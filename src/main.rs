mod app;
mod cache;
mod colordepth;
mod config;
mod cpu;
mod dive;
mod event;
mod feeds;
mod fx;
mod geo;
mod intercept;
mod keys;
mod lexicon;
mod paths;
mod reading;
mod source;
mod theme;
mod ui;

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use ratatui::DefaultTerminal;
use tokio::sync::{broadcast, mpsc};
use tracing_subscriber::EnvFilter;

use app::App;
use config::{Config, FxLevel};
use event::Msg;
use fx::{BootLine, Fx};
use keys::Keys;

/// Keep one previous log once the current one passes this size.
const LOG_ROTATE_BYTES: u64 = 5_000_000;

#[derive(Parser)]
#[command(
    name = "ghostwire",
    version,
    about = "Ambient netrunner rig: live public feeds in your terminal."
)]
struct Cli {
    /// Run on simulated feeds, with no network ("construct" mode).
    #[arg(long)]
    demo: bool,

    /// Config file to use instead of ~/.config/ghostwire/config.toml.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Effects level, overriding the config.
    #[arg(long, value_enum)]
    fx: Option<FxLevel>,

    /// Write a commented example config to the config path, then exit.
    #[arg(long)]
    init_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = match cli.config {
        Some(path) => path,
        None => paths::config_path()?,
    };
    if cli.init_config {
        return init_config(&config_path);
    }

    let log_path = init_logging()?;
    let (mut config, config_found) = Config::load(&config_path)?;
    if let Some(fx) = cli.fx {
        config.fx.level = fx;
    }
    tracing::info!(config = %config_path.display(), config_found, demo = cli.demo, "jacking in");
    let keys_path = keys::path_beside(&config_path);
    let (keys, keys_exposed) = Keys::load(&keys_path)?;
    if keys_exposed {
        tracing::warn!(
            "{} is readable by other users; chmod 600 it",
            keys_path.display()
        );
    }

    let http = reqwest::Client::builder()
        .user_agent(concat!("ghostwire/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (rebreach, _) = broadcast::channel(4);
    let cache = if cli.demo { None } else { open_cache() };
    let started = feeds::spawn_all(
        &config,
        &keys,
        cli.demo,
        &http,
        &tx,
        &rebreach,
        cache.as_ref(),
    );
    event::spawn_input(tx);
    let mut app = App::new(config, config_found, cli.demo, started, rebreach, cache);
    if keys_exposed {
        app.warnings.push(lexicon::KEYS_EXPOSED.into());
    }
    let boot = boot_log(&app, &config_path, &keys, keys_exposed);
    let depth = colordepth::Depth::detect();
    tracing::info!(?depth, "color depth");
    let mut fx = Fx::new(app.config.fx.level, boot, Instant::now(), depth);

    let mut terminal = ratatui::init();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle(lexicon::RIG)
    );
    let result = run(&mut terminal, &mut app, &mut fx, &mut rx).await;
    ratatui::restore();
    tracing::info!("jacked out");
    result.with_context(|| format!("see log at {}", log_path.display()))
}

async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    fx: &mut Fx,
    rx: &mut mpsc::UnboundedReceiver<Msg>,
) -> Result<()> {
    loop {
        let now = Instant::now();
        fx.on_frame(now, &app.ready_nodes());
        terminal.draw(|frame| ui::draw(frame, app, fx))?;
        let wait = app
            .frame_wait()
            .min(fx.frame_wait(now, ui::radar_on_screen(app)));
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(msg) => handle(app, fx, msg),
                None => return Ok(()),
            },
            _ = tokio::time::sleep(wait) => {}
        }
        while let Ok(msg) = rx.try_recv() {
            handle(app, fx, msg);
        }
        let now = Instant::now();
        app.on_tick(now);
        fx.absorb(app.take_signals(), now);
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Any key during the boot log skips it; after that, keys go to the app.
fn handle(app: &mut App, fx: &mut Fx, msg: Msg) {
    let now = Instant::now();
    match msg {
        Msg::Key(_) if fx.booting(now) => fx.skip_boot(now),
        msg => app.handle(msg),
    }
}

/// The jack-in log: facts about this run, in the rig's voice.
fn boot_log(app: &App, config_path: &Path, keys: &Keys, keys_exposed: bool) -> Vec<BootLine> {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    let short = |path: &Path| match home.as_ref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    let sector = &app.config.sector;
    let mut lines = vec![BootLine::new(
        "config",
        if app.config_found {
            short(config_path)
        } else {
            "none found, running on defaults".into()
        },
    )];
    if app.demo {
        lines.push(BootLine::new("construct", "simulated feeds, no network"));
    }
    lines.push(BootLine::new(
        "sector",
        match sector.fix() {
            Some((lat, lon)) => format!("{} at {lat:.2}, {lon:.2}", sector.name),
            None => format!("{}, no fix: ATMOS and SKYTRAFFIC dark", sector.name),
        },
    ));
    let mark = |present: bool| if present { "✓" } else { "—" };
    let mut key_line = format!(
        "finnhub {}  coingecko {}",
        mark(keys.finnhub().is_some()),
        mark(keys.coingecko().is_some())
    );
    if keys_exposed {
        key_line.push_str("  EXPOSED");
    }
    lines.push(BootLine::new("keys", key_line));
    let ghosts = app
        .sources
        .values()
        .filter(|s| s.link == source::Link::Ghost)
        .count();
    lines.push(BootLine::new(
        "ghost cache",
        match ghosts {
            0 => "cold".to_string(),
            n => format!("{n} readings"),
        },
    ));
    lines.push(BootLine::new(
        "node discovery",
        format!(
            "{} nodes, {} sources",
            source::NodeId::ALL.len(),
            app.sources.len()
        ),
    ));
    lines.push(BootLine::new(
        "fx",
        format!("{:?}", app.config.fx.level).to_lowercase(),
    ));
    lines
}

/// Running without a cache only costs the instant ghost display on the next start.
fn open_cache() -> Option<cache::Cache> {
    let dir = paths::cache_dir().ok()?.join("readings");
    cache::Cache::open(&dir)
        .inspect_err(|e| tracing::warn!("no reading cache: {e:#}"))
        .ok()
}

/// Writes whichever of config.toml and keys.toml don't exist yet; never overwrites.
fn init_config(config_path: &Path) -> Result<()> {
    let keys_path = keys::path_beside(config_path);
    let files = [
        (config_path, config::EXAMPLE, false),
        (keys_path.as_path(), keys::EXAMPLE, true),
    ];
    for (path, contents, private) in files {
        if path.exists() {
            println!("kept existing {}", path.display());
        } else {
            paths::write_new(path, contents, private)?;
            println!("wrote {}", path.display());
        }
    }
    println!("next: set [sector] lat/lon in config.toml, and your Finnhub key in keys.toml.");
    Ok(())
}

/// Logs go to a file because the TUI owns the terminal. Level comes from
/// `GHOSTWIRE_LOG` (default `info`).
fn init_logging() -> Result<PathBuf> {
    let path = paths::log_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening log at {}", path.display()))?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_env("GHOSTWIRE_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    Ok(path)
}

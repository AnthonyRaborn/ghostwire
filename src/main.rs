mod app;
mod cache;
mod config;
mod cpu;
mod event;
mod feeds;
mod fx;
mod geo;
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

use anyhow::{Context, Result};
use clap::Parser;
use ratatui::DefaultTerminal;
use tokio::sync::{broadcast, mpsc};
use tracing_subscriber::EnvFilter;

use app::App;
use config::{Config, FxLevel};
use event::Msg;
use keys::Keys;

/// Keep one previous log once the current one passes this size.
const LOG_ROTATE_BYTES: u64 = 5_000_000;

#[derive(Parser)]
#[command(
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

    let mut terminal = ratatui::init();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle(lexicon::RIG)
    );
    let result = run(&mut terminal, &mut app, &mut rx).await;
    ratatui::restore();
    tracing::info!("jacked out");
    result.with_context(|| format!("see log at {}", log_path.display()))
}

async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    rx: &mut mpsc::UnboundedReceiver<Msg>,
) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(msg) => app.handle(msg),
                None => return Ok(()),
            },
            _ = tokio::time::sleep(app.frame_wait()) => {}
        }
        while let Ok(msg) = rx.try_recv() {
            app.handle(msg);
        }
        app.on_tick();
        if app.should_quit {
            return Ok(());
        }
    }
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

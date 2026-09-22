use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};

/// `$XDG_CONFIG_HOME/ghostwire/config.toml`, falling back to `~/.config/ghostwire/`.
pub fn config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => BaseDirs::new()
            .context("could not resolve a home directory")?
            .home_dir()
            .join(".config"),
    };
    Ok(base.join("ghostwire").join("config.toml"))
}

/// Platform cache dir (`~/Library/Caches/ghostwire` on macOS).
pub fn cache_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "ghostwire").context("could not resolve a home directory")?;
    Ok(dirs.cache_dir().to_path_buf())
}

pub fn log_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("ghostwire.log"))
}

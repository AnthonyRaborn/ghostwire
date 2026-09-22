use std::io::Write;
use std::path::{Path, PathBuf};

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

/// Creates `path` and its directory, refusing to overwrite. A private file is
/// owner-only from the moment it exists, rather than chmod-ed after the fact.
pub fn write_new(path: &Path, contents: &str, private: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn private_files_are_owner_only_and_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ghostwire-paths-test-{}", std::process::id()));
        let path = dir.join("nested").join("keys.toml");
        write_new(&path, "a", true).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(write_new(&path, "b", true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

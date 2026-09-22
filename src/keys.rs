//! API keys, kept out of config.toml so the config can be shared freely. They're read
//! from `keys.toml` beside the config, and an environment variable overrides each one.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

pub const EXAMPLE: &str = include_str!("../keys.example.toml");
const FILE_NAME: &str = "keys.toml";

/// A key that can't end up in a log line or debug dump by accident.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keys {
    finnhub: Option<Secret>,
    coingecko: Option<Secret>,
}

impl Keys {
    /// Loads `path`; a missing file just means no keys. The bool is true when the file
    /// is readable by other users.
    pub fn load(path: &Path) -> Result<(Self, bool)> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false));
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let keys = Self::parse(&text).with_context(|| format!("in {}", path.display()))?;
        Ok((keys, exposed(path)))
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn finnhub(&self) -> Option<Secret> {
        resolve(std::env::var("FINNHUB_API_KEY").ok(), self.finnhub.as_ref())
    }

    pub fn coingecko(&self) -> Option<Secret> {
        resolve(
            std::env::var("COINGECKO_API_KEY").ok(),
            self.coingecko.as_ref(),
        )
    }
}

/// `keys.toml` in the same directory as the config file.
pub fn path_beside(config_path: &Path) -> PathBuf {
    config_path.with_file_name(FILE_NAME)
}

/// The environment wins over the file; blank values count as unset.
fn resolve(env: Option<String>, file: Option<&Secret>) -> Option<Secret> {
    env.map(Secret)
        .into_iter()
        .chain(file.cloned())
        .map(|s| Secret(s.0.trim().to_string()))
        .find(|s| !s.0.is_empty())
}

#[cfg(unix)]
fn exposed(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0)
}

#[cfg(not(unix))]
fn exposed(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> Secret {
        Secret(s.into())
    }

    #[test]
    fn example_parses_to_no_keys() {
        let keys = Keys::parse(EXAMPLE).unwrap();
        assert!(keys.finnhub.is_none() && keys.coingecko.is_none());
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Keys::parse("finhub = \"typo\"").is_err());
    }

    #[test]
    fn environment_overrides_file_and_blanks_are_unset() {
        let file = secret("from-file");
        assert_eq!(
            resolve(Some("from-env".into()), Some(&file)),
            Some(secret("from-env"))
        );
        assert_eq!(resolve(None, Some(&file)), Some(file.clone()));
        assert_eq!(resolve(Some("  ".into()), Some(&file)), Some(file));
        assert_eq!(resolve(None, Some(&secret(""))), None);
        assert_eq!(
            resolve(Some(" padded ".into()), None),
            Some(secret("padded"))
        );
    }

    #[test]
    fn debug_output_never_shows_a_key() {
        let keys = Keys::parse("finnhub = \"sk-very-secret\"").unwrap();
        let dump = format!("{keys:?}");
        assert!(!dump.contains("sk-very-secret"), "{dump}");
        assert!(dump.contains("[redacted]"));
    }

    #[cfg(unix)]
    #[test]
    fn flags_a_file_other_users_can_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ghostwire-keys-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "finnhub = \"k\"").unwrap();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let (keys, exposed) = Keys::load(&path).unwrap();
        assert!(exposed);
        assert_eq!(keys.finnhub, Some(secret("k")));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!Keys::load(&path).unwrap().1);

        assert!(!Keys::load(&dir.join("missing.toml")).unwrap().1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lives_beside_the_config() {
        let path = path_beside(Path::new("/home/x/.config/ghostwire/config.toml"));
        assert_eq!(path, Path::new("/home/x/.config/ghostwire/keys.toml"));
    }
}

//! Phase 1 configuration: a default save directory so commands can take a bare file
//! name (e.g. `PLAYER1.U1`) instead of a full path.
//!
//! This is a deliberate development convenience and will be replaced by a proper
//! configuration system that reads from the per-OS config directory (see `ROADMAP.md`).
//! For now the tool reads a `config.toml` from the current working directory (or the
//! path in the `FRINGE_RETRO_CONFIG` environment variable). The `FRINGE_RETRO_SAVE_DIR`
//! environment variable, if set, overrides the configured directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const CONFIG_ENV: &str = "FRINGE_RETRO_CONFIG";
const SAVE_DIR_ENV: &str = "FRINGE_RETRO_SAVE_DIR";
const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// The parsed `config.toml`. Unknown keys are ignored so the file can grow without
/// breaking older builds.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    games: Games,
}

#[derive(Debug, Default, Deserialize)]
struct Games {
    #[serde(default)]
    ultima1: GameConfig,
}

#[derive(Debug, Default, Deserialize)]
struct GameConfig {
    save_dir: Option<PathBuf>,
}

impl Config {
    /// Load configuration from `FRINGE_RETRO_CONFIG` or `./config.toml`, then apply the
    /// `FRINGE_RETRO_SAVE_DIR` override. A missing config file is not an error — it just
    /// yields an empty configuration.
    pub fn load() -> Result<Self> {
        let path = std::env::var_os(CONFIG_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE));

        let mut config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
        };

        // Environment variable overrides the configured directory.
        if let Some(dir) = std::env::var_os(SAVE_DIR_ENV) {
            config.games.ultima1.save_dir = Some(PathBuf::from(dir));
        }

        Ok(config)
    }

    /// Resolve a user-supplied path argument to an actual file path.
    ///
    /// Absolute paths and paths that include a directory component are used verbatim. A
    /// bare file name (e.g. `PLAYER1.U1`) is joined onto the configured save directory.
    pub fn resolve_save_path(&self, arg: &Path) -> Result<PathBuf> {
        let is_bare = !arg.is_absolute() && arg.components().count() <= 1;
        if !is_bare {
            return Ok(arg.to_path_buf());
        }

        match &self.games.ultima1.save_dir {
            Some(dir) => Ok(dir.join(arg)),
            None => anyhow::bail!(
                "'{}' is a bare file name, but no save directory is configured.\n\
                 Pass a full path, set `save_dir` under [games.ultima1] in {DEFAULT_CONFIG_FILE}, \
                 or set the {SAVE_DIR_ENV} environment variable.",
                arg.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_dir(dir: &str) -> Config {
        toml::from_str(&format!("[games.ultima1]\nsave_dir = \"{dir}\"\n")).unwrap()
    }

    #[test]
    fn absolute_path_passes_through() {
        let cfg = config_with_dir("/base");
        let resolved = cfg
            .resolve_save_path(Path::new("/somewhere/PLAYER1.U1"))
            .unwrap();
        assert_eq!(resolved, PathBuf::from("/somewhere/PLAYER1.U1"));
    }

    #[test]
    fn bare_name_joins_configured_dir() {
        let cfg = config_with_dir("/base/dir");
        let resolved = cfg.resolve_save_path(Path::new("PLAYER1.U1")).unwrap();
        assert_eq!(resolved, PathBuf::from("/base/dir/PLAYER1.U1"));
    }

    #[test]
    fn path_with_directory_component_passes_through() {
        let cfg = config_with_dir("/base");
        let resolved = cfg.resolve_save_path(Path::new("sub/PLAYER1.U1")).unwrap();
        assert_eq!(resolved, PathBuf::from("sub/PLAYER1.U1"));
    }

    #[test]
    fn bare_name_without_configured_dir_errors() {
        let cfg = Config::default();
        assert!(cfg.resolve_save_path(Path::new("PLAYER1.U1")).is_err());
    }

    #[test]
    fn empty_config_parses() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.games.ultima1.save_dir.is_none());
    }
}

//! Minimal reader for the shared `config.toml` — only the parts the map exporter needs: the
//! `[map] export_dir` and each `[games.<id>]` data directory. The main CLI owns the full
//! library manifest; this focused reader avoids a cross-binary dependency until the config
//! model graduates into `crates/core`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const CONFIG_ENV: &str = "FRINGE_RETRO_CONFIG";
const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// The subset of `config.toml` the map tooling reads. Unknown keys are ignored.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    map: MapSettings,
    #[serde(default)]
    games: BTreeMap<String, GameEntry>,
}

/// The `[map]` table.
#[derive(Debug, Default, Deserialize)]
struct MapSettings {
    /// Where exported map bundles are written / served from. `~` is expanded.
    export_dir: Option<PathBuf>,
}

/// The fields of a `[games.<id>]` table the exporter cares about.
#[derive(Debug, Default, Deserialize)]
struct GameEntry {
    save_dir: Option<PathBuf>,
    install_dir: Option<PathBuf>,
}

impl Config {
    /// Load from `$FRINGE_RETRO_CONFIG`, else `./config.toml`. A missing file yields defaults.
    pub fn load() -> Result<Self> {
        let path = std::env::var_os(CONFIG_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE));
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// The configured persistent map export root, with a leading `~` expanded.
    pub fn export_dir(&self) -> Option<PathBuf> {
        self.map.export_dir.as_deref().map(expand_tilde)
    }

    /// A game's data directory (its `save_dir`, falling back to `install_dir`), `~` expanded.
    pub fn game_input_dir(&self, id: &str) -> Option<PathBuf> {
        let entry = self.games.get(id)?;
        entry
            .save_dir
            .as_deref()
            .or(entry.install_dir.as_deref())
            .map(expand_tilde)
    }
}

/// Expand a leading `~` to the user's home directory (`$HOME`).
fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        [map]
        export_dir = "/maps"
        [games.ultima1]
        save_dir = "/games/u1"
        [games.ultima6]
        install_dir = "/games/u6"
    "#;

    #[test]
    fn reads_export_dir_and_game_paths() {
        let cfg: Config = toml::from_str(SAMPLE).unwrap();
        assert_eq!(cfg.export_dir(), Some(PathBuf::from("/maps")));
        assert_eq!(
            cfg.game_input_dir("ultima1"),
            Some(PathBuf::from("/games/u1"))
        );
        // Falls back to install_dir when save_dir is absent.
        assert_eq!(
            cfg.game_input_dir("ultima6"),
            Some(PathBuf::from("/games/u6"))
        );
        assert_eq!(cfg.game_input_dir("missing"), None);
    }

    #[test]
    fn missing_map_table_yields_none() {
        let cfg: Config = toml::from_str("[games.ultima1]\nsave_dir = \"/x\"\n").unwrap();
        assert_eq!(cfg.export_dir(), None);
    }
}

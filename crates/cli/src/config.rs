//! Application configuration: the game **library manifest**.
//!
//! Users describe which games they own, where each game's saves live, and (optionally)
//! the platform they came from. Commands can then refer to a game by a short identifier
//! (e.g. `fringe-retro inspect ultima2`) instead of a full path.
//!
//! The tool reads `config.toml` from the current working directory (or the path in the
//! `FRINGE_RETRO_CONFIG` environment variable). A missing file yields an empty manifest.
//! A proper per-OS config location is planned (see `ROADMAP.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fringe_retro_core::games::GameKind;
use serde::Deserialize;

const CONFIG_ENV: &str = "FRINGE_RETRO_CONFIG";
const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// The parsed manifest. Unknown keys are ignored so the file can grow without breaking
/// older builds.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Games keyed by user-chosen identifier (the `[games.<id>]` table name).
    #[serde(default)]
    games: BTreeMap<String, GameEntry>,
}

/// One game in the library manifest.
#[derive(Debug, Deserialize)]
struct GameEntry {
    /// Which built-in game/parser this entry uses. Defaults to the entry's identifier,
    /// so `[games.ultima1]` needs no `game` key.
    game: Option<String>,
    /// Directory that holds this game's save files.
    save_dir: Option<PathBuf>,
    /// Whether this game is active. Defaults to `true`.
    #[serde(default = "default_true")]
    enabled: bool,
    /// Where the game came from (gog / steam / dosbox / …). Descriptive only for now.
    platform: Option<String>,
    /// Where the game is installed. Descriptive only for now.
    install_dir: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

/// An enabled game resolved from the manifest, for display and path resolution.
pub struct ResolvedGame {
    pub id: String,
    pub kind: GameKind,
    pub save_dir: Option<PathBuf>,
    pub platform: Option<String>,
    pub install_dir: Option<PathBuf>,
}

impl GameEntry {
    /// The built-in game this entry maps to (its `game` field, or the identifier).
    fn resolve_kind(&self, id: &str) -> Result<GameKind> {
        let name = self.game.as_deref().unwrap_or(id);
        GameKind::from_id(name).ok_or_else(|| {
            let known: Vec<_> = GameKind::ALL.iter().map(|k| k.id()).collect();
            anyhow::anyhow!(
                "'{id}' uses unknown game type '{name}'. Known types: {}",
                known.join(", ")
            )
        })
    }
}

impl Config {
    /// Load the manifest from `FRINGE_RETRO_CONFIG` or `./config.toml`. A missing file is
    /// not an error — it yields an empty manifest.
    pub fn load() -> Result<Self> {
        let path = std::env::var_os(CONFIG_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE));

        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    /// The enabled entry for an identifier (case-insensitive), if any.
    fn enabled_entry(&self, id: &str) -> Option<(&String, &GameEntry)> {
        self.games
            .iter()
            .find(|(key, entry)| entry.enabled && key.eq_ignore_ascii_case(id))
    }

    /// Resolve a command target to a save-file path.
    ///
    /// The target is treated as a **game identifier** if it matches a configured, enabled
    /// game — optionally with a `:file` selector to pick a specific save file
    /// (e.g. `ultima3:PARTY.ULT`). Otherwise it is used verbatim as a **filesystem path**,
    /// so explicit paths always work.
    pub fn resolve_save_path(&self, arg: &Path) -> Result<PathBuf> {
        let text = arg
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("target is not valid UTF-8: {}", arg.display()))?;
        let (id, file) = match text.split_once(':') {
            Some((id, file)) => (id, Some(file)),
            None => (text, None),
        };

        if let Some((key, entry)) = self.enabled_entry(id) {
            let kind = entry.resolve_kind(key)?;
            let dir = entry.save_dir.as_ref().ok_or_else(|| {
                anyhow::anyhow!("game '{key}' has no `save_dir` set in {DEFAULT_CONFIG_FILE}")
            })?;
            let name = file.unwrap_or_else(|| kind.default_save_file());
            return Ok(dir.join(name));
        }

        // Not a known identifier: use the argument verbatim as a filesystem path.
        Ok(arg.to_path_buf())
    }

    /// All enabled games in the manifest, in identifier order.
    pub fn games(&self) -> Result<Vec<ResolvedGame>> {
        let mut games = Vec::new();
        for (id, entry) in &self.games {
            if !entry.enabled {
                continue;
            }
            let kind = entry.resolve_kind(id)?;
            games.push(ResolvedGame {
                id: id.clone(),
                kind,
                save_dir: entry.save_dir.clone(),
                platform: entry.platform.clone(),
                install_dir: entry.install_dir.clone(),
            });
        }
        Ok(games)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_str: &str) -> Config {
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn identifier_resolves_to_default_save_file() {
        let cfg = parse("[games.ultima2]\nsave_dir = \"/g/u2\"\n");
        let p = cfg.resolve_save_path(Path::new("ultima2")).unwrap();
        assert_eq!(p, PathBuf::from("/g/u2/PLAYER"));
    }

    #[test]
    fn identifier_with_file_selector() {
        let cfg = parse("[games.ultima3]\nsave_dir = \"/g/u3\"\n");
        let p = cfg
            .resolve_save_path(Path::new("ultima3:PARTY.ULT"))
            .unwrap();
        assert_eq!(p, PathBuf::from("/g/u3/PARTY.ULT"));
    }

    #[test]
    fn game_defaults_to_identifier() {
        // Back-compat: [games.ultima1] with no `game` key still resolves.
        let cfg = parse("[games.ultima1]\nsave_dir = \"/base\"\n");
        let p = cfg.resolve_save_path(Path::new("ultima1")).unwrap();
        assert_eq!(p, PathBuf::from("/base/PLAYER1.U1"));
    }

    #[test]
    fn custom_identifier_uses_game_field() {
        let cfg = parse("[games.u2-gog]\ngame = \"ultima2\"\nsave_dir = \"/g\"\n");
        let p = cfg.resolve_save_path(Path::new("u2-gog")).unwrap();
        assert_eq!(p, PathBuf::from("/g/PLAYER"));
    }

    #[test]
    fn unknown_token_is_treated_as_path() {
        let cfg = parse("[games.ultima2]\nsave_dir = \"/g/u2\"\n");
        assert_eq!(
            cfg.resolve_save_path(Path::new("/some/PLAYER")).unwrap(),
            PathBuf::from("/some/PLAYER")
        );
        assert_eq!(
            cfg.resolve_save_path(Path::new("PLAYER")).unwrap(),
            PathBuf::from("PLAYER")
        );
    }

    #[test]
    fn disabled_game_is_not_an_identifier() {
        let cfg = parse("[games.ultima2]\nenabled = false\nsave_dir = \"/g/u2\"\n");
        // Falls through to a verbatim path, and is excluded from the games list.
        assert_eq!(
            cfg.resolve_save_path(Path::new("ultima2")).unwrap(),
            PathBuf::from("ultima2")
        );
        assert!(cfg.games().unwrap().is_empty());
    }

    #[test]
    fn games_lists_enabled_entries() {
        let cfg = parse(
            "[games.ultima1]\nsave_dir = \"/a\"\n\n[games.ultima2]\nplatform = \"gog\"\nsave_dir = \"/b\"\n",
        );
        let games = cfg.games().unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].id, "ultima1");
        assert_eq!(games[0].kind, GameKind::Ultima1);
        assert_eq!(games[1].platform.as_deref(), Some("gog"));
    }

    #[test]
    fn unknown_game_type_errors() {
        let cfg = parse("[games.custom]\ngame = \"nope\"\nsave_dir = \"/x\"\n");
        assert!(cfg.resolve_save_path(Path::new("custom")).is_err());
        assert!(cfg.games().is_err());
    }

    #[test]
    fn save_dir_required_for_identifier() {
        let cfg = parse("[games.ultima2]\nplatform = \"gog\"\n");
        assert!(cfg.resolve_save_path(Path::new("ultima2")).is_err());
    }

    #[test]
    fn empty_config_parses() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.games().unwrap().is_empty());
    }
}

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

use anyhow::{anyhow, Context, Result};
use fringe_retro_core::backup::RetentionPolicy;
use fringe_retro_core::games::GameKind;
use serde::Deserialize;

use crate::detect::{self, DetectedGame};
use crate::library::Library;

const CONFIG_ENV: &str = "FRINGE_RETRO_CONFIG";
const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// The parsed manifest. Unknown keys are ignored so the file can grow without breaking
/// older builds.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Games keyed by user-chosen identifier (the `[games.<id>]` table name).
    #[serde(default)]
    games: BTreeMap<String, GameEntry>,
    /// The Save Library location (Phase 5).
    #[serde(default)]
    library: LibrarySettings,
    /// Automatic-backup retention policy (Phase 5).
    #[serde(default)]
    backups: BackupSettings,
    /// Installed-game detection settings (Phase 6).
    #[serde(default)]
    detect: DetectSettings,
}

/// The `[detect]` table.
#[derive(Debug, Default, Deserialize)]
struct DetectSettings {
    /// When true, scan for installed games on every run and make any not already configured
    /// usable in-memory (nothing is written to disk). Off by default.
    #[serde(default)]
    auto: bool,
}

/// The `[library]` table.
#[derive(Debug, Default, Deserialize)]
struct LibrarySettings {
    /// Where snapshots are stored. `~` is expanded to the home directory.
    path: Option<PathBuf>,
}

/// The `[backups]` table. A value of `0` (or an absent key) means "no limit".
#[derive(Debug, Default, Deserialize)]
struct BackupSettings {
    /// Keep at most this many of the most-recent backups per save.
    keep: Option<usize>,
    /// Delete backups older than this many days.
    max_age_days: Option<u64>,
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
    /// True for entries injected by auto-detection (never read from the file).
    #[serde(skip)]
    detected: bool,
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
    /// Whether this game came from auto-detection rather than the config file.
    pub detected: bool,
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

    /// An entry synthesized from a detected install.
    fn detected(game: &DetectedGame) -> Self {
        GameEntry {
            game: Some(game.kind.id().to_string()),
            save_dir: Some(game.save_dir.clone()),
            enabled: true,
            platform: Some(game.platform.to_string()),
            install_dir: Some(game.install_dir.clone()),
            detected: true,
        }
    }
}

impl Config {
    /// Load the manifest from `FRINGE_RETRO_CONFIG` or `./config.toml`. A missing file is
    /// not an error — it yields an empty manifest.
    pub fn load() -> Result<Self> {
        let path = config_path();

        let mut config: Config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
        };
        // When enabled, fold auto-detected installs into the in-memory manifest.
        if config.detect.auto {
            config.merge_detected_from(&detect::detect_games());
        }
        Ok(config)
    }

    /// Merge detected games not already configured (by kind) into the in-memory manifest.
    fn merge_detected_from(&mut self, found: &[DetectedGame]) {
        let configured = self.configured_kinds();
        for game in found {
            if configured.contains(&game.kind) {
                continue;
            }
            self.games
                .entry(game.kind.id().to_string())
                .or_insert_with(|| GameEntry::detected(game));
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
                detected: entry.detected,
            });
        }
        Ok(games)
    }

    /// Resolve a manifest identifier to a single enabled game (kind + save location).
    pub fn resolve_game(&self, id: &str) -> Result<ResolvedGame> {
        let (key, entry) = self
            .enabled_entry(id)
            .ok_or_else(|| anyhow!("no enabled game '{id}' in {DEFAULT_CONFIG_FILE}"))?;
        let kind = entry.resolve_kind(key)?;
        Ok(ResolvedGame {
            id: key.clone(),
            kind,
            save_dir: entry.save_dir.clone(),
            platform: entry.platform.clone(),
            install_dir: entry.install_dir.clone(),
            detected: entry.detected,
        })
    }

    /// Resolve a token to a built-in game kind: a manifest identifier, or a built-in game id
    /// (e.g. `ultima3`) directly. Used where only the game family matters (e.g. filtering).
    pub fn game_kind(&self, token: &str) -> Result<GameKind> {
        if let Some((key, entry)) = self.enabled_entry(token) {
            return entry.resolve_kind(key);
        }
        GameKind::from_id(token).ok_or_else(|| anyhow!("unknown game '{token}'"))
    }

    /// The configured Save Library, or an error if `[library] path` is unset.
    pub fn library(&self) -> Result<Library> {
        let path = self.library.path.as_ref().ok_or_else(|| {
            anyhow!("no Save Library configured; set `[library] path` in {DEFAULT_CONFIG_FILE}")
        })?;
        Ok(Library::new(expand_tilde(path)))
    }

    /// The automatic-backup retention policy (a `0` limit means unlimited).
    pub fn retention(&self) -> RetentionPolicy {
        RetentionPolicy {
            keep: self.backups.keep.filter(|&k| k > 0),
            max_age_days: self.backups.max_age_days.filter(|&d| d > 0),
        }
    }

    /// The set of built-in games already present in the manifest (enabled or not), used to
    /// avoid re-adding a game that's already configured.
    pub fn configured_kinds(&self) -> std::collections::HashSet<GameKind> {
        self.games
            .iter()
            .filter_map(|(id, entry)| entry.resolve_kind(id).ok())
            .collect()
    }
}

/// The path the manifest is read from: `FRINGE_RETRO_CONFIG`, or `./config.toml`.
pub fn config_path() -> PathBuf {
    std::env::var_os(CONFIG_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE))
}

/// Expand a leading `~` to the user's home directory (`$HOME`); other paths are unchanged.
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
    fn library_requires_a_configured_path() {
        let cfg = parse("[games.ultima1]\nsave_dir = \"/a\"\n");
        assert!(cfg.library().is_err()); // no [library] path set

        let cfg = parse("[library]\npath = \"/saves/lib\"\n");
        assert!(cfg.library().is_ok());
    }

    #[test]
    fn auto_detect_merges_only_unconfigured_games() {
        use crate::detect::DetectedGame;
        let mut cfg: Config = toml::from_str("[games.ultima4]\nsave_dir = \"/x\"\n").unwrap();
        let found = vec![
            DetectedGame {
                kind: GameKind::Ultima4, // already configured -> not merged
                platform: "gog",
                install_dir: PathBuf::from("/Applications/Ultima IV™.app"),
                save_dir: PathBuf::from("/Applications/Ultima IV™.app/Contents/Resources/game"),
                save_present: true,
            },
            DetectedGame {
                kind: GameKind::Ultima5, // new -> merged in-memory
                platform: "gog",
                install_dir: PathBuf::from("/Applications/Ultima V™.app"),
                save_dir: PathBuf::from("/Applications/Ultima V™.app/Contents/Resources/game"),
                save_present: false,
            },
        ];
        cfg.merge_detected_from(&found);

        let games = cfg.games().unwrap();
        let u4 = games.iter().find(|g| g.kind == GameKind::Ultima4).unwrap();
        assert!(!u4.detected); // config entry, untouched
        assert_eq!(u4.save_dir.as_deref(), Some(Path::new("/x")));
        let u5 = games.iter().find(|g| g.kind == GameKind::Ultima5).unwrap();
        assert!(u5.detected); // came from detection
        assert!(u5
            .save_dir
            .as_ref()
            .unwrap()
            .ends_with("Contents/Resources/game"));
    }

    #[test]
    fn game_kind_resolves_manifest_id_or_builtin_id() {
        // A custom manifest id maps through its `game` field...
        let cfg = parse("[games.u3-gog]\ngame = \"ultima3\"\nsave_dir = \"/g\"\n");
        assert_eq!(cfg.game_kind("u3-gog").unwrap(), GameKind::Ultima3);
        // ...and a built-in id resolves even without a manifest entry.
        assert_eq!(cfg.game_kind("ultima4").unwrap(), GameKind::Ultima4);
        assert!(cfg.game_kind("nope").is_err());
    }

    #[test]
    fn expand_tilde_uses_home() {
        std::env::set_var("HOME", "/home/tester");
        assert_eq!(
            expand_tilde(Path::new("~/Retro Saves")),
            PathBuf::from("/home/tester/Retro Saves")
        );
        assert_eq!(
            expand_tilde(Path::new("/abs/path")),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    fn retention_reads_backups_table_and_treats_zero_as_unlimited() {
        let cfg = parse("[backups]\nkeep = 5\nmax_age_days = 30\n");
        let r = cfg.retention();
        assert_eq!(r.keep, Some(5));
        assert_eq!(r.max_age_days, Some(30));

        // A zero limit means "no limit".
        let cfg = parse("[backups]\nkeep = 0\n");
        assert!(cfg.retention().keep.is_none());

        // No table at all: unlimited.
        assert!(parse("").retention().is_unlimited());
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

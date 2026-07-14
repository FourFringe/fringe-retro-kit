//! Best-effort detection of installed games and their save directories.
//!
//! Phase 6, macOS + GOG only. GOG wraps each DOS game in an application bundle under
//! `/Applications`, with the save files inside `Contents/Resources/game`. We scan those
//! roots for bundles whose name matches a known game, confirm the save directory exists, and
//! report what we find. Optionally we append the newly-found games to the config manifest.
//!
//! The design is deliberately structured so other platforms/stores (Steam, Windows, Linux)
//! can slot in later; today only GOG-on-macOS is wired up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use fringe_retro_core::backup;
use fringe_retro_core::games::GameKind;

use crate::config::{self, Config};

/// Where GOG keeps a game's save files, relative to its `.app` bundle.
const GOG_MAC_SAVE_SUBPATH: &str = "Contents/Resources/game";

/// A game found on disk, with the paths we resolved for it.
pub struct DetectedGame {
    pub kind: GameKind,
    /// The store the game came from (currently always `gog`).
    pub platform: &'static str,
    /// The game's install directory (the `.app` bundle).
    pub install_dir: PathBuf,
    /// The directory holding the save files.
    pub save_dir: PathBuf,
    /// Whether the game's default save file is already present.
    pub save_present: bool,
}

/// The result of appending detected games to the config manifest.
pub struct WriteOutcome {
    /// Games newly added to the manifest.
    pub added: Vec<GameKind>,
    /// The backup made of the previous config, if it already existed.
    pub backup: Option<PathBuf>,
}

/// Candidate GOG/macOS `.app` bundle names for a game (matched loosely; see [`normalize`]).
/// Games with no candidates (e.g. Steam-only Wasteland) aren't detected here yet.
fn gog_mac_app_names(kind: GameKind) -> &'static [&'static str] {
    match kind {
        GameKind::Ultima1 => &["Ultima I"],
        GameKind::Ultima2 => &["Ultima II"],
        GameKind::Ultima3 => &["Ultima III"],
        GameKind::Ultima4 => &["Ultima IV"],
        GameKind::Ultima5 => &["Ultima V"],
        GameKind::Wasteland => &[],
    }
}

/// The directories to scan for GOG apps on macOS.
fn default_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    roots
}

/// Reduce a bundle name to lowercase alphanumerics, so `"Ultima IV™"`, `"Ultima IV.app"`, and
/// `"Ultima IV"` all compare equal.
fn normalize(name: &str) -> String {
    name.trim_end_matches(".app")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Detect installed games using the default macOS roots.
pub fn detect_games() -> Vec<DetectedGame> {
    detect_in_roots(&default_roots())
}

/// Detect games by scanning the given roots (injectable for tests).
fn detect_in_roots(roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut found = Vec::new();
    let mut seen: HashSet<GameKind> = HashSet::new();

    for kind in GameKind::ALL {
        let candidates = gog_mac_app_names(kind);
        if candidates.is_empty() {
            continue;
        }
        let wanted: Vec<String> = candidates.iter().map(|c| normalize(c)).collect();

        'roots: for root in roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".app") || !wanted.contains(&normalize(&name)) {
                    continue;
                }
                let install_dir = entry.path();
                let save_dir = install_dir.join(GOG_MAC_SAVE_SUBPATH);
                if save_dir.is_dir() && seen.insert(kind) {
                    let save_present = save_dir.join(kind.default_save_file()).exists();
                    found.push(DetectedGame {
                        kind,
                        platform: "gog",
                        install_dir,
                        save_dir,
                        save_present,
                    });
                    break 'roots;
                }
            }
        }
    }
    found
}

/// Append any detected games not already in `config` to the manifest at `config_path`,
/// backing up the existing file first. Games already configured (by kind) are skipped.
pub fn write_missing(
    config: &Config,
    found: &[DetectedGame],
    config_path: &Path,
) -> Result<WriteOutcome> {
    let configured = config.configured_kinds();
    let missing: Vec<&DetectedGame> = found
        .iter()
        .filter(|g| !configured.contains(&g.kind))
        .collect();
    if missing.is_empty() {
        return Ok(WriteOutcome {
            added: Vec::new(),
            backup: None,
        });
    }

    // Back up the existing config before touching it.
    let backup = if config_path.exists() {
        Some(backup::create(config_path).map_err(|e| anyhow!("{e}"))?)
    } else {
        None
    };

    let stamp = chrono::Local::now().format("%Y-%m-%d");
    let mut block = format!("\n# Added by `fringe-retro detect --write` ({stamp})\n");
    for g in &missing {
        block.push_str(&format!(
            "[games.{}]\nplatform = \"{}\"\nsave_dir = \"{}\"\n\n",
            g.kind.id(),
            g.platform,
            g.save_dir.display(),
        ));
    }

    let mut text = std::fs::read_to_string(config_path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&block);
    fringe_retro_core::save::atomic_write(config_path, text.as_bytes())
        .map_err(|e| anyhow!("{e}"))?;

    Ok(WriteOutcome {
        added: missing.iter().map(|g| g.kind).collect(),
        backup,
    })
}

/// The config path detection writes to (honors `FRINGE_RETRO_CONFIG`).
pub fn manifest_path() -> PathBuf {
    config::config_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create `<root>/<bundle>.app/Contents/Resources/game`, optionally with a save file.
    fn make_gog_app(root: &Path, bundle: &str, save_file: Option<&str>) {
        let game = root
            .join(format!("{bundle}.app"))
            .join(GOG_MAC_SAVE_SUBPATH);
        std::fs::create_dir_all(&game).unwrap();
        if let Some(f) = save_file {
            std::fs::write(game.join(f), b"x").unwrap();
        }
    }

    #[test]
    fn normalize_ignores_trademark_and_case() {
        assert_eq!(normalize("Ultima IV™"), "ultimaiv");
        assert_eq!(normalize("Ultima IV.app"), "ultimaiv");
        assert_eq!(normalize("ULTIMA iv"), "ultimaiv");
    }

    #[test]
    fn detects_gog_bundles_and_notes_save_presence() {
        let root = tempfile::tempdir().unwrap();
        make_gog_app(root.path(), "Ultima IV™", Some("PARTY.SAV"));
        make_gog_app(root.path(), "Ultima V™", None); // installed, not yet played
        std::fs::create_dir_all(root.path().join("Safari.app")).unwrap(); // unrelated app

        let found = detect_in_roots(&[root.path().to_path_buf()]);
        assert_eq!(found.len(), 2);

        let u4 = found.iter().find(|g| g.kind == GameKind::Ultima4).unwrap();
        assert_eq!(u4.platform, "gog");
        assert!(u4.save_present);
        assert!(u4.save_dir.ends_with("Contents/Resources/game"));

        let u5 = found.iter().find(|g| g.kind == GameKind::Ultima5).unwrap();
        assert!(!u5.save_present);
    }

    #[test]
    fn ignores_matching_apps_without_a_save_dir() {
        let root = tempfile::tempdir().unwrap();
        // Right name, but not a DOSBox bundle (no Contents/Resources/game).
        std::fs::create_dir_all(root.path().join("Ultima IV™.app")).unwrap();
        assert!(detect_in_roots(&[root.path().to_path_buf()]).is_empty());
    }

    #[test]
    fn write_missing_appends_only_new_games_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "[games.ultima4]\nsave_dir = \"/x\"\n").unwrap();
        let config: Config = toml::from_str("[games.ultima4]\nsave_dir = \"/x\"\n").unwrap();

        let found = vec![
            DetectedGame {
                kind: GameKind::Ultima4, // already configured -> skipped
                platform: "gog",
                install_dir: PathBuf::from("/Applications/Ultima IV™.app"),
                save_dir: PathBuf::from("/Applications/Ultima IV™.app/Contents/Resources/game"),
                save_present: true,
            },
            DetectedGame {
                kind: GameKind::Ultima5, // new -> appended
                platform: "gog",
                install_dir: PathBuf::from("/Applications/Ultima V™.app"),
                save_dir: PathBuf::from("/Applications/Ultima V™.app/Contents/Resources/game"),
                save_present: false,
            },
        ];

        let outcome = write_missing(&config, &found, &cfg).unwrap();
        assert_eq!(outcome.added, vec![GameKind::Ultima5]);
        assert!(outcome.backup.is_some()); // existing config was backed up

        let written = std::fs::read_to_string(&cfg).unwrap();
        assert!(written.contains("[games.ultima5]"));
        assert!(written.contains("Ultima V™.app"));
        // The pre-existing entry is preserved and not duplicated.
        assert_eq!(written.matches("[games.ultima4]").count(), 1);
    }

    #[test]
    fn write_missing_is_a_noop_when_all_configured() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "[games.ultima5]\nsave_dir = \"/x\"\n").unwrap();
        let config: Config = toml::from_str("[games.ultima5]\nsave_dir = \"/x\"\n").unwrap();
        let found = vec![DetectedGame {
            kind: GameKind::Ultima5,
            platform: "gog",
            install_dir: PathBuf::from("/a"),
            save_dir: PathBuf::from("/a/game"),
            save_present: true,
        }];
        let outcome = write_missing(&config, &found, &cfg).unwrap();
        assert!(outcome.added.is_empty());
        assert!(outcome.backup.is_none()); // nothing to do, no backup
    }
}

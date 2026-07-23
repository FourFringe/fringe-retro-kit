//! Best-effort detection of installed games and their save directories.
//!
//! Phase 6, macOS only. Two stores are wired up:
//! - **GOG** wraps each DOS game in an application bundle under `/Applications`, with the
//!   save files inside `Contents/Resources/game`.
//! - **Steam** records installs in `steamapps/appmanifest_<appid>.acf` (libraries listed in
//!   `libraryfolders.vdf`); a game's saves may live elsewhere (e.g. Wasteland saves to
//!   `~/Library/Application Support/Wasteland/<slot>`).
//!
//! We confirm each known game is installed, resolve its save directory, and report what we
//! find. Optionally we append the newly-found games to the config manifest. The design is
//! structured so more stores/platforms can slot in later.

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
    /// The store the game came from (`gog` or `steam`).
    pub platform: &'static str,
    /// The game's install directory.
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
        GameKind::Ultima6 => &["Ultima VI"],
        GameKind::Wasteland => &[],
        GameKind::BardsTale => &[],
    }
}

/// The save directory within a GOG bundle, relative to the `.app`. Most games save into the
/// DOSBox game directory; Ultima VI keeps its object files in a `SAVEGAME` subdirectory.
fn gog_mac_save_subpath(kind: GameKind) -> &'static str {
    match kind {
        GameKind::Ultima6 => "Contents/Resources/game/SAVEGAME",
        _ => GOG_MAC_SAVE_SUBPATH,
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
    let mut found = detect_in_roots(&default_roots());
    let mut seen: HashSet<GameKind> = found.iter().map(|g| g.kind).collect();
    if let (Some(steam), Some(home)) = (default_steam_root(), home_dir()) {
        for game in detect_steam(&steam, &home) {
            if seen.insert(game.kind) {
                found.push(game);
            }
        }
    }
    found
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn default_steam_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Library/Application Support/Steam"))
}

/// Detect games by scanning the given roots (injectable for tests).
fn detect_in_roots(roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut found = Vec::new();
    for kind in GameKind::ALL {
        let names = gog_mac_app_names(kind);
        if names.is_empty() {
            continue;
        }
        if let Some(install_dir) = find_gog_bundle(roots, names) {
            // Confirm it's a DOSBox bundle by finding the save directory.
            let save_dir = install_dir.join(gog_mac_save_subpath(kind));
            if save_dir.is_dir() {
                let save_present = save_dir.join(kind.default_save_file()).exists();
                found.push(DetectedGame {
                    kind,
                    platform: "gog",
                    install_dir,
                    save_dir,
                    save_present,
                });
            }
        }
    }
    found
}

/// The first `.app` bundle under `roots` whose name matches one of `names` (loosely; see
/// [`normalize`]).
fn find_gog_bundle(roots: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    let wanted: Vec<String> = names.iter().map(|n| normalize(n)).collect();
    if wanted.is_empty() {
        return None;
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".app") && wanted.contains(&normalize(&name)) {
                return Some(entry.path());
            }
        }
    }
    None
}

// --- Steam ---------------------------------------------------------------------------

/// The Steam app id for a game, if we know how to detect it via Steam.
fn steam_app_id(kind: GameKind) -> Option<u32> {
    match kind {
        GameKind::Wasteland => Some(259130), // "Wasteland 1 - The Original Classic"
        GameKind::BardsTale => Some(843260), // "The Bard's Tale Trilogy"
        _ => None,
    }
}

/// Where a Steam game keeps its saves (which is often *not* the install directory), resolving
/// any active save slot. `home` is the user's home directory.
fn steam_save_dir(kind: GameKind, home: &Path) -> Option<PathBuf> {
    match kind {
        GameKind::Wasteland => wasteland_slot(&home.join("Library/Application Support/Wasteland")),
        GameKind::BardsTale => bardstale_save_dir(&home.join("Library/Application Support/Steam")),
        _ => None,
    }
}

/// The Bard's Tale Trilogy stores its saves in Steam Cloud, under a per-user `userdata`
/// directory: `userdata/<steamid>/843260/remote/saves`. We don't know the numeric steam id
/// ahead of time, so scan for the first such directory that exists.
fn bardstale_save_dir(steam_root: &Path) -> Option<PathBuf> {
    let userdata = steam_root.join("userdata");
    let entries = std::fs::read_dir(&userdata).ok()?;
    for entry in entries.flatten() {
        let saves = entry.path().join("843260/remote/saves");
        if saves.is_dir() {
            return Some(saves);
        }
    }
    None
}

/// The active Wasteland save slot under `root`: the one named by `LASTSAVE`, else the first
/// slot directory that contains a `GAME1` save.
pub(crate) fn wasteland_slot(root: &Path) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    // The real slot directories (those holding a GAME1 save), in stable order.
    let mut slots: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("GAME1").is_file())
        .collect();
    slots.sort();
    // Prefer the slot named by LASTSAVE (matched case-insensitively; the folder is usually
    // upper-cased while LASTSAVE stores the party name as typed).
    if let Ok(last) = std::fs::read_to_string(root.join("LASTSAVE")) {
        let name = last.trim();
        if let Some(hit) = slots.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) {
            return Some(hit.clone());
        }
    }
    slots.into_iter().next()
}

/// Steam library roots: the default root plus any listed in `libraryfolders.vdf`.
fn steam_libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam_root.to_path_buf()];
    let vdf = steam_root.join("steamapps/libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf) {
        for line in text.lines() {
            if let Some(path) = vdf_value(line, "path") {
                let path = PathBuf::from(path);
                if !libs.contains(&path) {
                    libs.push(path);
                }
            }
        }
    }
    libs
}

/// Find the app manifest for `app_id` across `libraries`, returning its library and the
/// game's `installdir`.
fn steam_manifest(libraries: &[PathBuf], app_id: u32) -> Option<(PathBuf, String)> {
    for lib in libraries {
        let acf = lib.join(format!("steamapps/appmanifest_{app_id}.acf"));
        if let Ok(text) = std::fs::read_to_string(&acf) {
            let installdir = text
                .lines()
                .find_map(|l| vdf_value(l, "installdir"))
                .unwrap_or_default();
            return Some((lib.clone(), installdir));
        }
    }
    None
}

/// Extract the quoted value following `"key"` on a VDF/ACF line, e.g. `"path"  "/x"` -> `/x`.
fn vdf_value(line: &str, key: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(&format!("\"{key}\""))?;
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].to_string())
}

/// Detect Steam-installed games, given the Steam root and the user's home directory.
fn detect_steam(steam_root: &Path, home: &Path) -> Vec<DetectedGame> {
    let libraries = steam_libraries(steam_root);
    let mut found = Vec::new();
    for kind in GameKind::ALL {
        let Some(app_id) = steam_app_id(kind) else {
            continue;
        };
        let Some((lib, installdir)) = steam_manifest(&libraries, app_id) else {
            continue;
        };
        let Some(save_dir) = steam_save_dir(kind, home) else {
            continue;
        };
        if !save_dir.is_dir() {
            continue;
        }
        let save_present = save_dir.join(kind.default_save_file()).is_file();
        found.push(DetectedGame {
            kind,
            platform: "steam",
            install_dir: lib.join("steamapps/common").join(installdir),
            save_dir,
            save_present,
        });
    }
    found
}

// --- Recognized-but-unsupported games ------------------------------------------------

/// A game we can *recognize* on disk but don't support editing yet (detect-only).
pub struct UnsupportedGame {
    pub title: String,
    pub platform: &'static str,
    pub install_dir: PathBuf,
}

/// Detection signature for a not-yet-supported game.
struct UnsupportedSig {
    title: &'static str,
    gog_names: &'static [&'static str],
    steam_app_id: Option<u32>,
}

/// Games we recognize but can't edit yet — surfaced by `detect --all` so you can see what's
/// installed and request support. (Feel free to grow this list. Don't add games that already
/// have a `GameKind`; those are reported in the supported section.)
const UNSUPPORTED: &[UnsupportedSig] = &[
    UnsupportedSig {
        title: "The Bard's Tale Trilogy",
        gog_names: &["The Bard's Tale Trilogy", "Bard's Tale Trilogy"],
        steam_app_id: Some(843260),
    },
    UnsupportedSig {
        title: "Magic Carpet Plus",
        gog_names: &["Magic Carpet Plus", "Magic Carpet"],
        steam_app_id: None,
    },
    UnsupportedSig {
        title: "Magic Carpet 2",
        gog_names: &["Magic Carpet 2"],
        steam_app_id: None,
    },
];

/// Detect recognized-but-unsupported games installed on this machine.
pub fn detect_unsupported() -> Vec<UnsupportedGame> {
    let steam_libs = default_steam_root()
        .map(|r| steam_libraries(&r))
        .unwrap_or_default();
    detect_unsupported_in(&default_roots(), &steam_libs)
}

/// Detect unsupported games against injected GOG roots and Steam libraries (for tests).
fn detect_unsupported_in(roots: &[PathBuf], steam_libs: &[PathBuf]) -> Vec<UnsupportedGame> {
    let mut found = Vec::new();
    for sig in UNSUPPORTED {
        // A Steam app manifest is an authoritative signal, so prefer it over a name-matched
        // `.app` bundle (which may just be a leftover launcher).
        if let Some((lib, installdir)) = sig
            .steam_app_id
            .and_then(|id| steam_manifest(steam_libs, id))
        {
            found.push(UnsupportedGame {
                title: sig.title.to_string(),
                platform: "steam",
                install_dir: lib.join("steamapps/common").join(installdir),
            });
        } else if let Some(install_dir) = find_gog_bundle(roots, sig.gog_names) {
            found.push(UnsupportedGame {
                title: sig.title.to_string(),
                platform: "gog",
                install_dir,
            });
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

    /// Build a fake Steam library (with a Wasteland manifest) and a Wasteland save slot under
    /// one temp "home", returning the steam root and home path.
    fn fake_steam_home(with_lastsave: bool) -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join("Library/Application Support/Steam");
        std::fs::create_dir_all(steam.join("steamapps/common/Wasteland")).unwrap();
        std::fs::write(
            steam.join("steamapps/appmanifest_259130.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\"259130\"\n\t\"installdir\"\t\"Wasteland\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
                steam.display()
            ),
        )
        .unwrap();

        let ws = home.path().join("Library/Application Support/Wasteland");
        let slot = ws.join("ENKI");
        std::fs::create_dir_all(&slot).unwrap();
        std::fs::write(slot.join("GAME1"), b"save").unwrap();
        if with_lastsave {
            std::fs::write(ws.join("LASTSAVE"), "Enki\r\n").unwrap();
        }
        home
    }

    #[test]
    fn detects_steam_wasteland() {
        let home = fake_steam_home(true);
        let steam = home.path().join("Library/Application Support/Steam");

        let found = detect_steam(&steam, home.path());
        assert_eq!(found.len(), 1);
        let w = &found[0];
        assert_eq!(w.kind, GameKind::Wasteland);
        assert_eq!(w.platform, "steam");
        assert!(w.save_present);
        assert!(w.save_dir.ends_with("Wasteland/ENKI"));
        assert!(w.install_dir.ends_with("steamapps/common/Wasteland"));
    }

    #[test]
    fn wasteland_slot_falls_back_without_lastsave() {
        let home = fake_steam_home(false); // no LASTSAVE file
        let root = home.path().join("Library/Application Support/Wasteland");
        assert_eq!(wasteland_slot(&root).unwrap(), root.join("ENKI"));
    }

    #[test]
    fn steam_not_detected_without_manifest() {
        // A Wasteland save exists, but the game isn't registered with Steam.
        let home = tempfile::tempdir().unwrap();
        let slot = home
            .path()
            .join("Library/Application Support/Wasteland/ENKI");
        std::fs::create_dir_all(&slot).unwrap();
        std::fs::write(slot.join("GAME1"), b"save").unwrap();
        let steam = home.path().join("Library/Application Support/Steam");
        assert!(detect_steam(&steam, home.path()).is_empty());
    }

    #[test]
    fn vdf_value_extracts_quoted_values() {
        assert_eq!(
            vdf_value("\t\"path\"\t\"/a/b\"", "path").as_deref(),
            Some("/a/b")
        );
        assert_eq!(
            vdf_value("  \"installdir\"  \"Wasteland\"", "installdir").as_deref(),
            Some("Wasteland")
        );
        assert_eq!(vdf_value("\"other\" \"x\"", "path"), None);
    }

    #[test]
    fn detects_unsupported_gog_and_steam() {
        let apps = tempfile::tempdir().unwrap();
        // A GOG bundle we recognize but don't support (no save subdir needed).
        std::fs::create_dir_all(apps.path().join("Magic Carpet Plus™.app")).unwrap();

        // A Steam library with the Bard's Tale Trilogy manifest.
        let steam = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(steam.path().join("steamapps")).unwrap();
        std::fs::write(
            steam.path().join("steamapps/appmanifest_843260.acf"),
            "\"AppState\"\n{\n\t\"installdir\"\t\"The Bard's Tale Trilogy\"\n}\n",
        )
        .unwrap();

        let found =
            detect_unsupported_in(&[apps.path().to_path_buf()], &[steam.path().to_path_buf()]);
        let titles: Vec<&str> = found.iter().map(|g| g.title.as_str()).collect();
        assert!(titles.contains(&"Magic Carpet Plus"));
        assert!(titles.contains(&"The Bard's Tale Trilogy"));

        let mc = found
            .iter()
            .find(|g| g.title == "Magic Carpet Plus")
            .unwrap();
        assert_eq!(mc.platform, "gog");
        let bt = found
            .iter()
            .find(|g| g.title == "The Bard's Tale Trilogy")
            .unwrap();
        assert_eq!(bt.platform, "steam");
        assert!(bt
            .install_dir
            .ends_with("steamapps/common/The Bard's Tale Trilogy"));
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

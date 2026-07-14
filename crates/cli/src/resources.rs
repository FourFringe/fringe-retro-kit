//! Curated web resources (links) per game.
//!
//! A default set ships with the tool (embedded from the repo's `resources.toml`). Users can
//! add or override links by setting `FRINGE_RETRO_RESOURCES` to their own file, or by placing
//! a `resources.toml` in the working directory. User links whose URL already exists for that
//! game are skipped, so the defaults are never duplicated.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

const RESOURCES_ENV: &str = "FRINGE_RETRO_RESOURCES";
const DEFAULT_RESOURCES_FILE: &str = "resources.toml";

/// The default resource list shipped with the tool (the repo-root `resources.toml`).
const BUNDLED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources.toml"));

/// A single web resource for a game.
#[derive(Debug, Clone, Deserialize)]
pub struct Resource {
    /// Human-readable title shown in listings.
    pub title: String,
    /// The URL to open.
    pub url: String,
    /// A free-form grouping label (e.g. `wiki`, `walkthrough`, `map`, `format`, `play`).
    #[serde(default)]
    pub category: String,
}

/// The `[resources]` table: game id -> list of links.
#[derive(Debug, Default, Deserialize)]
struct ResourceFile {
    #[serde(default)]
    resources: BTreeMap<String, Vec<Resource>>,
}

/// All known resources, keyed by game id.
pub struct Resources {
    by_game: BTreeMap<String, Vec<Resource>>,
}

impl Resources {
    /// Load the bundled defaults, then merge any user overrides. User links whose URL already
    /// exists for a game are skipped.
    pub fn load() -> Result<Self> {
        let mut by_game = parse(BUNDLED)
            .context("bundled resources.toml is invalid")?
            .resources;
        if let Some((path, text)) = user_file()? {
            let extra =
                parse(&text).with_context(|| format!("failed to parse {}", path.display()))?;
            for (game, links) in extra.resources {
                let entry = by_game.entry(game).or_default();
                for link in links {
                    if !entry.iter().any(|r| r.url == link.url) {
                        entry.push(link);
                    }
                }
            }
        }
        Ok(Self { by_game })
    }

    /// The resources for a game id (empty if none are known).
    pub fn for_game(&self, id: &str) -> &[Resource] {
        self.by_game.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The bundled defaults only, ignoring any user file. Infallible fallback for the TUI so a
    /// malformed user file never blanks out the built-in links.
    pub fn bundled() -> Self {
        let by_game = parse(BUNDLED)
            .expect("bundled resources.toml is valid")
            .resources;
        Self { by_game }
    }

    /// Every game id that has at least one resource, in a stable order.
    pub fn games(&self) -> impl Iterator<Item = &str> {
        self.by_game.keys().map(String::as_str)
    }
}

/// Parse a resources file's text.
fn parse(text: &str) -> Result<ResourceFile> {
    Ok(toml::from_str(text)?)
}

/// Locate and read the optional user resources file (env var, else `./resources.toml`).
fn user_file() -> Result<Option<(PathBuf, String)>> {
    let path = std::env::var_os(RESOURCES_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_RESOURCES_FILE));
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some((path, text))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Open a URL in the operating system's default browser.
pub fn open_url(url: &str) -> Result<()> {
    open::that(url).with_context(|| format!("failed to open {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_resources_parse_and_cover_the_ultimas() {
        let res = Resources::load().unwrap();
        for id in ["ultima1", "ultima2", "ultima3", "ultima4", "ultima5"] {
            assert!(
                !res.for_game(id).is_empty(),
                "expected bundled resources for {id}"
            );
        }
    }

    #[test]
    fn every_bundled_link_has_a_title_and_https_url() {
        let res = Resources::load().unwrap();
        for id in res.games().map(str::to_owned).collect::<Vec<_>>() {
            for link in res.for_game(&id) {
                assert!(!link.title.is_empty(), "{id} link missing title");
                assert!(
                    link.url.starts_with("https://"),
                    "{id} link '{}' is not https",
                    link.title
                );
            }
        }
    }

    #[test]
    fn unknown_game_has_no_resources() {
        let res = Resources::load().unwrap();
        assert!(res.for_game("nonesuch").is_empty());
    }
}

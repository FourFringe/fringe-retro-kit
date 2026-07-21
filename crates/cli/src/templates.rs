//! Character **templates**: named, pre-baked sets of field values that can be applied on
//! top of an existing character.
//!
//! Templates are read from `templates.toml` (or the path in the `FRINGE_RETRO_TEMPLATES`
//! environment variable). Each `[[template]]` names a game, a label, an optional
//! description, and a `fields` map of field key → value (using the same value syntax as
//! the editor). Applying a template is nothing more than a batch of validated field edits
//! against the currently loaded character — see [`crate::edit::Session::apply`].
//!
//! A missing file is not an error (it yields an empty set). Templates are *not* validated
//! at load time; validity depends on the target game's schema and is checked when a
//! template is applied or listed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const TEMPLATES_ENV: &str = "FRINGE_RETRO_TEMPLATES";
const DEFAULT_TEMPLATES_FILE: &str = "templates.toml";

#[derive(Debug, Default, Deserialize)]
struct TemplatesFile {
    #[serde(default)]
    template: Vec<RawTemplate>,
}

#[derive(Debug, Deserialize)]
struct RawTemplate {
    game: String,
    name: String,
    description: Option<String>,
    #[serde(default)]
    fields: BTreeMap<String, toml::Value>,
}

/// A named set of field values for a game's character.
#[derive(Debug, Clone)]
pub struct Template {
    /// The game identifier this template targets (e.g. `ultima2`).
    pub game: String,
    /// A short label shown in listings.
    pub name: String,
    /// An optional human-readable description.
    pub description: Option<String>,
    /// Field key → value, as strings in the same syntax the editor accepts.
    pub fields: Vec<(String, String)>,
}

/// All templates read from the templates file.
#[derive(Debug, Default)]
pub struct TemplateSet {
    templates: Vec<Template>,
}

impl TemplateSet {
    /// Load templates from `FRINGE_RETRO_TEMPLATES` or `./templates.toml`. A missing file
    /// is not an error — it yields an empty set.
    pub fn load() -> Result<Self> {
        let path = std::env::var_os(TEMPLATES_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_TEMPLATES_FILE));

        let raw: TemplatesFile = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => TemplatesFile::default(),
            Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
        };

        let templates = raw
            .template
            .into_iter()
            .map(|t| Template {
                game: t.game,
                name: t.name,
                description: t.description,
                fields: t
                    .fields
                    .into_iter()
                    .map(|(k, v)| (k, value_to_string(&v)))
                    .collect(),
            })
            .collect();
        Ok(TemplateSet { templates })
    }

    /// Every template, in file order.
    pub fn all(&self) -> &[Template] {
        &self.templates
    }

    /// Build a set directly from templates (used by the interactive UI and tests).
    #[cfg(test)]
    pub fn from_templates(templates: Vec<Template>) -> Self {
        TemplateSet { templates }
    }

    /// The templates targeting the given game identifier (case-insensitive), in file order.
    pub fn for_game(&self, game_id: &str) -> Vec<&Template> {
        self.templates
            .iter()
            .filter(|t| t.game.eq_ignore_ascii_case(game_id))
            .collect()
    }
}

/// The path the templates file is read from (and written to when capturing).
pub fn templates_path() -> PathBuf {
    std::env::var_os(TEMPLATES_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_TEMPLATES_FILE))
}

/// Append a new `[[template]]` block to the templates file at `path`, creating the file if
/// needed. Existing content is preserved verbatim (this never rewrites the file), so any
/// comments or hand-authored templates are left untouched.
pub fn append_template(
    path: &Path,
    game: &str,
    name: &str,
    fields: &[(String, String)],
) -> Result<()> {
    use std::io::Write as _;

    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut block = String::new();
    // Ensure a blank line separates the new block from any existing content.
    if !existing.is_empty() && !existing.ends_with('\n') {
        block.push('\n');
    }
    block.push('\n');
    block.push_str("[[template]]\n");
    block.push_str(&format!("game = {}\n", toml_string(game)));
    block.push_str(&format!("name = {}\n", toml_string(name)));
    let pairs: Vec<String> = fields
        .iter()
        // Quote every key: field keys can contain characters that aren't valid in a TOML bare
        // key (a colon in `ammo:1`, a space in `Alarm Disarm`), which would corrupt the file.
        .map(|(k, v)| format!("{} = {}", toml_string(k), toml_scalar(v)))
        .collect();
    block.push_str(&format!("fields = {{ {} }}\n", pairs.join(", ")));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(block.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Quote and escape a string as a TOML basic string.
fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Render a captured field value: a bare integer when it's all digits, else a quoted string.
fn toml_scalar(v: &str) -> String {
    if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
        v.to_string()
    } else {
        toml_string(v)
    }
}

/// Render a TOML value as the string the editor's field parser expects.
fn value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(n) => n.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        // Arrays/tables/datetimes aren't valid field values; stringify so validation
        // rejects them with a clear error rather than silently dropping them.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_to_string_covers_scalars() {
        assert_eq!(value_to_string(&toml::Value::Integer(35)), "35");
        assert_eq!(
            value_to_string(&toml::Value::String("Sword".into())),
            "Sword"
        );
        assert_eq!(value_to_string(&toml::Value::Boolean(true)), "true");
    }

    #[test]
    fn parses_and_filters_by_game() {
        let text = r#"
            [[template]]
            game = "ultima2"
            name = "Fighter"
            description = "melee"
            fields = { strength = 35, weapon = "Sword" }

            [[template]]
            game = "ultima1"
            name = "Rich"
            fields = { gold = 500 }
        "#;
        let raw: TemplatesFile = toml::from_str(text).unwrap();
        let set = TemplateSet {
            templates: raw
                .template
                .into_iter()
                .map(|t| Template {
                    game: t.game,
                    name: t.name,
                    description: t.description,
                    fields: t
                        .fields
                        .into_iter()
                        .map(|(k, v)| (k, value_to_string(&v)))
                        .collect(),
                })
                .collect(),
        };

        assert_eq!(set.all().len(), 2);
        let u2 = set.for_game("ULTIMA2");
        assert_eq!(u2.len(), 1);
        assert_eq!(u2[0].name, "Fighter");
        // Fields preserve their string/number rendering.
        assert!(u2[0]
            .fields
            .iter()
            .any(|(k, v)| k == "weapon" && v == "Sword"));
    }

    #[test]
    fn append_writes_reloadable_blocks_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("templates.toml");
        std::fs::write(&path, "# my templates\n").unwrap();

        append_template(
            &path,
            "ultima1",
            "Rich",
            &[
                ("gold".into(), "500".into()),
                ("name".into(), "Enki".into()),
            ],
        )
        .unwrap();
        append_template(&path, "ultima2", "Fed", &[("food".into(), "3000".into())]).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // The hand-authored comment is preserved.
        assert!(text.contains("# my templates"));
        // Keys are quoted; numeric values are bare, string values quoted.
        assert!(text.contains("\"gold\" = 500"));
        assert!(text.contains("\"name\" = \"Enki\""));

        // Both blocks parse back into templates.
        let raw: TemplatesFile = toml::from_str(&text).unwrap();
        assert_eq!(raw.template.len(), 2);
    }

    #[test]
    fn append_quotes_keys_with_special_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("templates.toml");
        // A colon (`ammo:1`) or space (`Alarm Disarm`) isn't valid in a TOML bare key, so the
        // key must be quoted or the whole file fails to parse (the "none found" bug).
        append_template(
            &path,
            "wasteland",
            "Reload",
            &[
                ("con".into(), "95".into()),
                ("ammo:1".into(), "50".into()),
                ("Alarm Disarm".into(), "3".into()),
            ],
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"ammo:1\" = 50"));
        assert!(text.contains("\"Alarm Disarm\" = 3"));
        // It parses back with every field intact.
        let raw: TemplatesFile = toml::from_str(&text).unwrap();
        assert_eq!(raw.template.len(), 1);
        assert_eq!(raw.template[0].fields.len(), 3);
    }
}

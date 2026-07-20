//! Interactive terminal UI (Phase 4), built with Ratatui + Crossterm using a small
//! Elm-style state/update/view loop. Launched when `fringe-retro` is run with no command.
//!
//! Iteration 1 is a read-only **browser**: a list of the games in your library manifest,
//! and a scrollable inspector for the selected game's save. Editing and the Save Library
//! come later (see `ROADMAP.md`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use fringe_retro_core::backup;
use fringe_retro_core::backup::RetentionPolicy;
use fringe_retro_core::games::GameKind;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::edit::{Entity, FieldRow, Session};
use crate::library::Library;
use crate::resources::{self, Resources};
use crate::templates::TemplateSet;

/// One save file a game may keep in its save directory.
struct SaveFile {
    name: String,
    path: PathBuf,
    found: bool,
}

/// One game shown in the browser.
struct GameRow {
    id: String,
    title: String,
    kind: GameKind,
    inspectable: bool,
    /// The game's save directory (from the manifest), if configured.
    save_dir: Option<PathBuf>,
    /// The game's candidate save files (default first).
    files: Vec<SaveFile>,
}

impl GameRow {
    /// Whether any of this game's save files exist on disk.
    fn found(&self) -> bool {
        self.files.iter().any(|f| f.found)
    }
}

/// Which screen the browser is showing. The browser keeps a stack of these; the top is
/// the current view and `Esc` pops back to the previous one.
enum Screen {
    /// The list of games (selection is an index into `App::games`).
    Games(ListState),
    /// A chooser for a game's save files (shown when a game has more than one).
    SaveFiles(FileList),
    /// A list of a multi-character save's characters (roster slots / party members).
    Characters(CharList),
    /// A field editor for one character.
    Edit(Editor),
    /// A browser of the current save's timestamped backups, with a preview of each.
    Backups(BackupList),
    /// A picker of character templates to apply to the current character.
    Templates(TemplateList),
    /// A list of curated web links for a game (opened in the OS browser).
    Resources(ResourceList),
    /// A browser/manager for a game's Save Library snapshots.
    Library(LibraryList),
    /// A read-only message (unsupported games, errors).
    Inspect(Inspector),
    /// An unsaved-changes prompt.
    Confirm(Confirm),
}

/// A selectable list of a save's characters.
struct CharList {
    title: String,
    entities: Vec<Entity>,
    list: ListState,
    /// A transient one-line message (e.g. the result of a restore).
    status: Option<String>,
}

/// A chooser for one of a game's save files (e.g. Ultima III roster vs. party).
struct FileList {
    title: String,
    entries: Vec<FileEntry>,
    list: ListState,
}

/// One selectable save file in the [`FileList`].
struct FileEntry {
    label: String,
    path: PathBuf,
}

/// A list of a game's curated web resources.
struct ResourceList {
    game_title: String,
    entries: Vec<ResourceItem>,
    list: ListState,
    /// A transient one-line message (e.g. the result of opening a link).
    status: Option<String>,
}

/// One selectable web resource in the [`ResourceList`].
struct ResourceItem {
    title: String,
    url: String,
    category: String,
}

/// A browser/manager for a game's Save Library snapshots (list + decoded preview).
struct LibraryList {
    title: String,
    kind: GameKind,
    /// The game's active save directory (needed to add/restore); `None` if unconfigured.
    save_dir: Option<PathBuf>,
    entries: Vec<SnapshotItem>,
    list: ListState,
    preview: Inspector,
    status: Option<String>,
    /// `Some` while typing a name (add / rename / duplicate).
    input: Option<LibraryInput>,
}

/// One snapshot shown in the [`LibraryList`].
struct SnapshotItem {
    name: String,
    slug: String,
    notes: Option<String>,
    updated: Option<String>,
    label: String,
    dir: PathBuf,
    files: Vec<String>,
}

/// An in-progress name entry in the library browser.
struct LibraryInput {
    action: LibraryInputAction,
    buffer: String,
}

/// Which name-taking library operation is being entered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryInputAction {
    Add,
    Rename,
    Duplicate,
}

impl LibraryList {
    /// The currently selected snapshot, if any.
    fn selected(&self) -> Option<&SnapshotItem> {
        self.list.selected().and_then(|i| self.entries.get(i))
    }
}

/// One timestamped backup file of the current save.
struct BackupEntry {
    path: PathBuf,
    label: String,
    is_current: bool,
}

/// A browser of the current save's backups: a list on the left, a decoded preview of the
/// selected backup on the right.
struct BackupList {
    title: String,
    /// The editor entity to refresh after a restore reloads the session.
    entity: usize,
    /// The active save these are backups of (for the "changes since this backup" diff).
    save_path: PathBuf,
    entries: Vec<BackupEntry>,
    list: ListState,
    preview: Inspector,
    /// A transient one-line message (e.g. the result of a snapshot).
    status: Option<String>,
}

/// One template shown in the picker, with the result of validating it against the game.
struct TemplateItem {
    name: String,
    description: Option<String>,
    fields: Vec<(String, String)>,
    /// `Some(reason)` when the template is invalid for this game and cannot be applied.
    error: Option<String>,
}

/// A picker of character templates for the current game: a list on the left, a preview of
/// the selected template's fields (and any validation error) on the right.
struct TemplateList {
    title: String,
    /// The editor entity (character) a chosen template is applied to.
    entity: usize,
    entries: Vec<TemplateItem>,
    list: ListState,
    preview: Inspector,
    status: Option<String>,
}

/// A field editor for one character within the current session.
struct Editor {
    /// Index of the entity (character) within the session.
    entity: usize,
    title: String,
    rows: Vec<FieldRow>,
    /// The rendered rows: section headers interleaved with fields. Selection indexes this.
    display: Vec<DisplayRow>,
    list: ListState,
    /// `Some` while the selected field's value is being typed.
    input: Option<String>,
    /// `Some` while choosing an enum/letter/boolean field's value from a list.
    picker: Option<Picker>,
    /// A one-line message (last edit, validation error, or save result).
    status: Option<String>,
    /// Field keys changed this session (manual edits or applied templates); used to
    /// pre-check fields when capturing a template.
    edited: BTreeSet<&'static str>,
    /// `Some` while capturing the current character's fields into a new template.
    capture: Option<Capture>,
    /// `Some` while choosing an item to add to this character's list.
    add_item: Option<ItemAdd>,
}

/// In-progress capture of the current character's fields into a new template.
struct Capture {
    /// One flag per editor row (parallel to `Editor::rows`): whether it's included.
    selected: Vec<bool>,
    /// `Some(buf)` while typing the new template's name.
    naming: Option<String>,
}

/// In-progress selection of an enum/letter/boolean field's value from an ordered list.
struct Picker {
    options: Vec<String>,
    index: usize,
}

/// In-progress "add item" picker: a type-to-filter, scrollable list of the game's items.
struct ItemAdd {
    /// The full catalog `(id, name)`.
    catalog: Vec<(u8, &'static str)>,
    /// Case-insensitive substring filter.
    filter: String,
    /// Indices into `catalog` that match the filter.
    matches: Vec<usize>,
    list: ListState,
}

impl ItemAdd {
    fn new(catalog: Vec<(u8, &'static str)>) -> Self {
        let mut picker = ItemAdd {
            catalog,
            filter: String::new(),
            matches: Vec::new(),
            list: ListState::default(),
        };
        picker.refilter();
        picker
    }

    /// Recompute `matches` from the current filter and reset the selection to the top.
    fn refilter(&mut self) {
        let needle = self.filter.to_ascii_lowercase();
        self.matches = self
            .catalog
            .iter()
            .enumerate()
            .filter(|(_, (_, name))| {
                needle.is_empty() || name.to_ascii_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        self.list.select((!self.matches.is_empty()).then_some(0));
    }

    /// The highlighted `(id, name)`, if any.
    fn selected(&self) -> Option<(u8, &'static str)> {
        self.catalog
            .get(*self.matches.get(self.list.selected()?)?)
            .copied()
    }
}

/// One rendered row of the editor: either a (non-selectable) section header or a field
/// (an index into `Editor::rows`).
enum DisplayRow {
    Header(&'static str),
    Field(usize),
}

/// Build the rendered rows for a field list, inserting a header whenever the section
/// changes. Fields without a section produce no headers (a flat list).
fn build_display(rows: &[FieldRow]) -> Vec<DisplayRow> {
    let mut display = Vec::new();
    let mut current: Option<&'static str> = None;
    for (i, row) in rows.iter().enumerate() {
        if let Some(section) = row.section {
            if current != Some(section) {
                display.push(DisplayRow::Header(section));
                current = Some(section);
            }
        }
        display.push(DisplayRow::Field(i));
    }
    display
}

impl Editor {
    /// The `rows` index of the currently selected field, if a field (not a header) is
    /// selected.
    fn selected_field(&self) -> Option<usize> {
        match self.list.selected().and_then(|i| self.display.get(i)) {
            Some(DisplayRow::Field(i)) => Some(*i),
            _ => None,
        }
    }

    fn selected_row(&self) -> Option<&FieldRow> {
        self.selected_field().and_then(|i| self.rows.get(i))
    }

    /// Move the selection by `delta`, skipping section headers and wrapping around.
    fn move_selection(&mut self, delta: i32) {
        let n = self.display.len();
        if n == 0 {
            return;
        }
        let mut idx = self.list.selected().unwrap_or(0) as i32;
        for _ in 0..n {
            idx = (idx + delta).rem_euclid(n as i32);
            if matches!(self.display[idx as usize], DisplayRow::Field(_)) {
                self.list.select(Some(idx as usize));
                return;
            }
        }
    }

    /// Begin editing the selected field. Enum/letter/boolean fields open a value picker;
    /// everything else opens a free-text input seeded with the current value.
    fn begin_edit(&mut self) {
        if let Some(row) = self.selected_row() {
            if let Some(options) = row.pick_options() {
                let index = options
                    .iter()
                    .position(|o| o.eq_ignore_ascii_case(&row.value))
                    .unwrap_or(0);
                self.picker = Some(Picker { options, index });
                self.status = None;
            } else {
                self.input = Some(row.value.clone());
                self.status = None;
            }
        }
    }

    /// Begin capturing a template, pre-checking the fields edited this session.
    fn begin_capture(&mut self) {
        let selected = self
            .rows
            .iter()
            .map(|r| self.edited.contains(r.key))
            .collect();
        self.capture = Some(Capture {
            selected,
            naming: None,
        });
        self.status = None;
    }
}

/// What to do once a modal prompt is resolved.
#[derive(Clone)]
enum Pending {
    QuitApp,
    LeaveFile,
    /// Restore the given backup over the current save, then refresh the editor entity.
    Restore {
        backup: PathBuf,
        entity: usize,
    },
    /// Restore the named library snapshot into the active save directory.
    LibraryRestore {
        slug: String,
    },
    /// Delete the named library snapshot.
    LibraryDelete {
        slug: String,
    },
}

/// A modal prompt (unsaved-changes guard, or a restore confirmation).
struct Confirm {
    pending: Pending,
    message: Option<String>,
}

/// A scrollable inspection view of one save.
struct Inspector {
    title: String,
    content: Vec<String>,
    scroll: u16,
    /// Height (in lines) of the text area at the last draw, used for paging.
    viewport: u16,
}

impl Inspector {
    fn new(title: String, content: Vec<String>) -> Self {
        Inspector {
            title,
            content,
            scroll: 0,
            viewport: 0,
        }
    }

    /// The largest scroll offset that still fills the viewport with content.
    fn max_scroll(&self) -> u16 {
        let visible = self.viewport.max(1) as usize;
        self.content.len().saturating_sub(visible) as u16
    }

    /// A page step, keeping one line of overlap for context.
    fn page(&self) -> u16 {
        self.viewport.saturating_sub(1).max(1)
    }

    fn scroll_by(&mut self, delta: i32) {
        let max = self.max_scroll() as i32;
        self.scroll = (self.scroll as i32 + delta).clamp(0, max) as u16;
    }

    fn scroll_down(&mut self) {
        self.scroll_by(1);
    }

    fn scroll_up(&mut self) {
        self.scroll_by(-1);
    }

    fn page_down(&mut self) {
        self.scroll_by(self.page() as i32);
    }

    fn page_up(&mut self) {
        self.scroll_by(-(self.page() as i32));
    }

    fn home(&mut self) {
        self.scroll = 0;
    }

    fn end(&mut self) {
        self.scroll = self.max_scroll();
    }
}

/// The browser application state: the game list, an optional open editing session, and a
/// stack of screens (the top is the current view).
struct App {
    games: Vec<GameRow>,
    templates: TemplateSet,
    /// A load error for the templates file, surfaced when the picker is opened.
    template_error: Option<String>,
    /// Curated web links per game (bundled defaults plus any user overrides).
    resources: Resources,
    /// The Save Library, if `[library] path` is configured.
    library: Option<Library>,
    /// Automatic-backup retention policy.
    retention: RetentionPolicy,
    session: Option<Session>,
    stack: Vec<Screen>,
    running: bool,
}

/// What a keypress asked the app to do, applied after the screen borrow is released.
enum Action {
    None,
    Back,
    Quit,
    OpenGame(Option<usize>),
    OpenSaveFile(Option<usize>),
    OpenEntry(Option<usize>),
    Commit(String),
    Save,
    AddItem,
    AddItemCommit(u8),
    OpenBackups,
    RequestRestore,
    Snapshot,
    OpenTemplates,
    ApplyTemplate,
    SaveTemplate(String),
    OpenResources(Option<usize>),
    OpenResourceLink(Option<usize>),
    OpenLibrary(Option<usize>),
    LibraryRestoreRequest,
    LibraryDeleteRequest,
    LibraryBeginInput(LibraryInputAction),
    LibraryCommitInput(String),
    ConfirmSave,
    ConfirmDiscard,
    ConfirmAccept,
    ConfirmCancel,
}

impl App {
    fn new(games: Vec<GameRow>) -> Self {
        let mut list = ListState::default();
        if !games.is_empty() {
            list.select(Some(0));
        }
        App {
            games,
            templates: TemplateSet::default(),
            template_error: None,
            resources: Resources::bundled(),
            library: None,
            retention: RetentionPolicy::default(),
            session: None,
            stack: vec![Screen::Games(list)],
            running: true,
        }
    }

    fn session_dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.is_dirty())
    }

    /// Handle a "go back" request, guarding unsaved edits when leaving the current file.
    fn back(&mut self) {
        if self.stack.len() <= 1 {
            self.request_quit();
            return;
        }
        // Returning to a chooser (the games list or the file chooser) leaves the open file.
        let below_is_chooser = matches!(
            self.stack.get(self.stack.len() - 2),
            Some(Screen::Games(_)) | Some(Screen::SaveFiles(_))
        );
        let leaving_file = below_is_chooser && self.session.is_some();
        if leaving_file && self.session_dirty() {
            self.stack.push(Screen::Confirm(Confirm {
                pending: Pending::LeaveFile,
                message: None,
            }));
            return;
        }
        self.stack.pop();
        if leaving_file {
            self.session = None;
        }
    }

    /// Handle a "quit" request, guarding unsaved edits.
    fn request_quit(&mut self) {
        if self.session_dirty() {
            self.stack.push(Screen::Confirm(Confirm {
                pending: Pending::QuitApp,
                message: None,
            }));
        } else {
            self.running = false;
        }
    }

    fn confirm_save(&mut self) {
        let result = self.session.as_mut().map(|s| s.save());
        match result {
            Some(Ok(_)) | None => self.complete_pending(),
            Some(Err(e)) => {
                if let Some(Screen::Confirm(c)) = self.stack.last_mut() {
                    c.message = Some(format!("Save failed: {e}"));
                }
            }
        }
    }

    fn confirm_discard(&mut self) {
        self.complete_pending();
    }

    fn confirm_cancel(&mut self) {
        if matches!(self.stack.last(), Some(Screen::Confirm(_))) {
            self.stack.pop();
        }
    }

    /// Pop the confirm prompt and carry out its pending action.
    fn complete_pending(&mut self) {
        let pending = match self.stack.last() {
            Some(Screen::Confirm(c)) => Some(c.pending.clone()),
            _ => None,
        };
        if matches!(self.stack.last(), Some(Screen::Confirm(_))) {
            self.stack.pop();
        }
        match pending {
            Some(Pending::QuitApp) => self.running = false,
            Some(Pending::LeaveFile) => {
                // The confirm was already popped; pop the content screen we were leaving.
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
                self.session = None;
            }
            Some(Pending::Restore { backup, entity }) => self.do_restore(backup, entity),
            Some(Pending::LibraryRestore { slug }) => self.do_library_restore(slug),
            Some(Pending::LibraryDelete { slug }) => self.do_library_delete(slug),
            None => {}
        }
    }

    /// Open the backup browser for the current session's save, previewing the newest backup.
    fn open_backups(&mut self) {
        let (path, entity, editor_title) = match (self.session.as_ref(), self.stack.last()) {
            (Some(s), Some(Screen::Edit(ed))) => {
                (s.path().to_path_buf(), ed.entity, ed.title.clone())
            }
            (Some(s), Some(Screen::Characters(cl))) => {
                // A backup captures the whole save file (all characters and party state), so
                // it's reachable straight from the member list. Carry the highlighted entity
                // only so a later-opened editor can be refreshed after a restore.
                let entity = cl
                    .list
                    .selected()
                    .and_then(|i| cl.entities.get(i))
                    .map_or(0, |e| e.index);
                (s.path().to_path_buf(), entity, cl.title.clone())
            }
            _ => return,
        };
        let entries = build_backup_entries(&path);
        let mut list = ListState::default();
        if !entries.is_empty() {
            list.select(Some(0));
        }
        let content = match entries.first() {
            Some(e) => backup_preview(&e.path, &path),
            None => vec!["(no backups yet)".to_string()],
        };
        let preview = Inspector::new("Preview".to_string(), content);
        self.stack.push(Screen::Backups(BackupList {
            title: format!("Backups — {editor_title}"),
            entity,
            save_path: path,
            entries,
            list,
            preview,
            status: None,
        }));
    }

    /// Ask to confirm restoring the selected backup over the current save.
    fn request_restore(&mut self) {
        let target = match self.stack.last() {
            Some(Screen::Backups(bl)) => bl
                .list
                .selected()
                .and_then(|i| bl.entries.get(i))
                .map(|e| (e.path.clone(), bl.entity)),
            _ => None,
        };
        let Some((backup, entity)) = target else {
            return;
        };
        let prompt = if self.session_dirty() {
            "Restore this backup? Unsaved edits will be lost."
        } else {
            "Restore this backup over the current save?"
        };
        let mut lines = vec![prompt.to_string(), String::new()];
        // Preview what restoring would change (current on-disk save -> backup).
        match self
            .session
            .as_ref()
            .map(|s| crate::compare::compare(s.path(), &backup))
        {
            Some(Ok(crate::compare::Comparison::Identical)) => {
                lines.push("This backup matches the current save.".to_string());
            }
            Some(Ok(comparison)) => {
                lines.push("Restoring will change:".to_string());
                let diff_lines = crate::compare::report(&comparison);
                const MAX: usize = 16;
                if diff_lines.len() > MAX {
                    lines.extend(diff_lines.into_iter().take(MAX));
                    lines.push("  …".to_string());
                } else {
                    lines.extend(diff_lines);
                }
            }
            _ => {}
        }
        self.stack.push(Screen::Confirm(Confirm {
            pending: Pending::Restore { backup, entity },
            message: Some(lines.join("\n")),
        }));
    }

    /// Restore a backup over the current save, reload the session, and refresh the editor.
    fn do_restore(&mut self, backup: PathBuf, entity: usize) {
        let Some(path) = self.session.as_ref().map(|s| s.path().to_path_buf()) else {
            return;
        };
        let outcome = backup::restore(&backup, &path);
        self.prune_backups(&path);
        // Return to the editor beneath the backup browser.
        if matches!(self.stack.last(), Some(Screen::Backups(_))) {
            self.stack.pop();
        }
        let status = match outcome {
            Ok(Some(pre)) => {
                if let Ok(Some(session)) = Session::load(&path) {
                    self.session = Some(session);
                }
                format!("Restored. Previous save backed up to {}", pre.display())
            }
            Ok(None) => "Already at this version; nothing changed.".to_string(),
            Err(e) => format!("Restore failed: {e}"),
        };
        let rows = self.session.as_ref().map(|s| s.rows(entity));
        let entities = self.session.as_ref().map(|s| s.entities());
        match self.stack.last_mut() {
            Some(Screen::Edit(ed)) => {
                if let Some(rows) = rows {
                    ed.rows = rows;
                }
                ed.input = None;
                ed.status = Some(status);
            }
            // Restore was launched from the member list: refresh it (names/summaries may
            // have changed) and report the outcome there.
            Some(Screen::Characters(cl)) => {
                if let Some(entities) = entities {
                    let sel = cl.list.selected().unwrap_or(0);
                    cl.entities = entities;
                    if !cl.entities.is_empty() {
                        cl.list.select(Some(sel.min(cl.entities.len() - 1)));
                    }
                }
                cl.status = Some(status);
            }
            _ => {}
        }
    }

    /// Snapshot the current save on disk into a new backup (a manual "bookmark"), skipping
    /// it when an identical backup already exists. Refreshes the backup list in place.
    fn snapshot_current(&mut self) {
        let Some(path) = self.session.as_ref().map(|s| s.path().to_path_buf()) else {
            return;
        };
        let (status, changed) = match backup::snapshot(&path) {
            Ok(Some(p)) => (format!("Snapshot saved: {}", backup_stamp(&p, &path)), true),
            Ok(None) => (
                "An identical backup already exists; no snapshot made.".to_string(),
                false,
            ),
            Err(e) => (format!("Snapshot failed: {e}"), false),
        };
        if changed {
            self.prune_backups(&path);
        }
        if let Some(Screen::Backups(bl)) = self.stack.last_mut() {
            if changed {
                bl.entries = build_backup_entries(&path);
                bl.list.select((!bl.entries.is_empty()).then_some(0));
                refresh_backup_preview(bl);
            }
            bl.status = Some(status);
        }
    }

    /// Open the template picker for the current game, validating each template against it.
    fn open_templates(&mut self) {
        let (kind, entity, editor_title) = match (self.session.as_ref(), self.stack.last()) {
            (Some(s), Some(Screen::Edit(ed))) => (s.kind(), ed.entity, ed.title.clone()),
            _ => return,
        };
        if let Some(err) = &self.template_error {
            let msg = vec!["Could not read templates:".to_string(), err.clone()];
            self.stack.push(Screen::Inspect(Inspector::new(
                "Templates".to_string(),
                msg,
            )));
            return;
        }
        let templates = self.templates.for_game(kind.id());
        if templates.is_empty() {
            let msg = vec![
                format!("No templates defined for {}.", kind.title()),
                String::new(),
                "Add some to templates.toml (see templates.example.toml).".to_string(),
            ];
            self.stack.push(Screen::Inspect(Inspector::new(
                "Templates".to_string(),
                msg,
            )));
            return;
        }
        let entries: Vec<TemplateItem> = templates
            .iter()
            .map(|t| {
                let error = match Session::scratch(kind) {
                    Some(mut scratch) => scratch
                        .apply(entity, &t.fields)
                        .err()
                        .map(|e| e.to_string()),
                    None => Some("game is not editable".to_string()),
                };
                TemplateItem {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    fields: t.fields.clone(),
                    error,
                }
            })
            .collect();
        let mut list = ListState::default();
        list.select(Some(0));
        let content = template_preview(&entries[0]);
        let preview = Inspector::new("Preview".to_string(), content);
        self.stack.push(Screen::Templates(TemplateList {
            title: format!("Templates — {editor_title}"),
            entity,
            entries,
            list,
            preview,
            status: None,
        }));
    }

    /// Apply the selected template's fields to the current character (in memory), unless it
    /// is invalid. Marks the session dirty; the user still saves manually.
    fn apply_template_selected(&mut self) {
        let selected = match self.stack.last() {
            Some(Screen::Templates(tl)) => tl
                .list
                .selected()
                .and_then(|i| tl.entries.get(i))
                .map(|e| (e.name.clone(), e.fields.clone(), e.error.clone(), tl.entity)),
            _ => None,
        };
        let Some((name, fields, error, entity)) = selected else {
            return;
        };
        if let Some(err) = error {
            if let Some(Screen::Templates(tl)) = self.stack.last_mut() {
                tl.status = Some(format!("Cannot apply '{name}': {err}"));
            }
            return;
        }
        let result = self.session.as_mut().map(|s| s.apply(entity, &fields));
        // Return to the editor beneath the picker.
        if matches!(self.stack.last(), Some(Screen::Templates(_))) {
            self.stack.pop();
        }
        let applied_ok = matches!(result, Some(Ok(())));
        let status = match result {
            Some(Ok(())) => format!(
                "Applied '{name}' ({} field(s)). Press s to save.",
                fields.len()
            ),
            Some(Err(e)) => format!("Apply failed: {e}"),
            None => "No character loaded.".to_string(),
        };
        let rows = self.session.as_ref().map(|s| s.rows(entity));
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            if let Some(rows) = rows {
                ed.rows = rows;
            }
            if applied_ok {
                let keys: std::collections::HashSet<&str> =
                    fields.iter().map(|(k, _)| k.as_str()).collect();
                for r in &ed.rows {
                    if keys.contains(r.key) {
                        ed.edited.insert(r.key);
                    }
                }
            }
            ed.input = None;
            ed.status = Some(status);
        }
    }

    /// Write the fields selected in capture mode as a new template, then reload the set.
    fn save_template(&mut self, name: String) {
        let (game, fields) = match (self.session.as_ref(), self.stack.last()) {
            (Some(s), Some(Screen::Edit(ed))) => {
                let Some(cap) = &ed.capture else {
                    return;
                };
                let fields: Vec<(String, String)> = ed
                    .rows
                    .iter()
                    .zip(&cap.selected)
                    .filter(|(_, selected)| **selected)
                    .map(|(r, _)| (r.key.to_string(), r.value.clone()))
                    .collect();
                (s.kind().id().to_string(), fields)
            }
            _ => return,
        };
        let path = crate::templates::templates_path();
        let outcome = if fields.is_empty() {
            Err("no fields selected".to_string())
        } else {
            crate::templates::append_template(&path, &game, &name, &fields)
                .map_err(|e| e.to_string())
        };
        if outcome.is_ok() {
            if let Ok(set) = crate::templates::TemplateSet::load() {
                self.templates = set;
                self.template_error = None;
            }
        }
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            ed.capture = None;
            ed.status = Some(match outcome {
                Ok(()) => format!(
                    "Saved template '{name}' ({} field(s)) → {}",
                    fields.len(),
                    path.display()
                ),
                Err(e) => format!("Save template failed: {e}"),
            });
        }
    }

    /// Open a list of the selected game's curated web resources.
    fn open_resources(&mut self, index: usize) {
        let Some(row) = self.games.get(index) else {
            return;
        };
        let links = self.resources.for_game(&row.id);
        if links.is_empty() {
            self.stack.push(Screen::Inspect(Inspector::new(
                format!("{} — Resources", row.title),
                vec![
                    String::new(),
                    format!("  No web resources are configured for {}.", row.title),
                ],
            )));
            return;
        }
        let entries: Vec<ResourceItem> = links
            .iter()
            .map(|r| ResourceItem {
                title: r.title.clone(),
                url: r.url.clone(),
                category: r.category.clone(),
            })
            .collect();
        let mut list = ListState::default();
        list.select(Some(0));
        self.stack.push(Screen::Resources(ResourceList {
            game_title: row.title.clone(),
            entries,
            list,
            status: None,
        }));
    }

    /// Open the selected web resource in the operating system's default browser.
    fn open_resource_link(&mut self, index: usize) {
        if let Some(Screen::Resources(rl)) = self.stack.last_mut() {
            if let Some(item) = rl.entries.get(index) {
                rl.status = Some(match resources::open_url(&item.url) {
                    Ok(()) => format!("Opened {} in your browser", item.title),
                    Err(e) => format!("Could not open link: {e}"),
                });
            }
        }
    }

    /// Open the Save Library browser for the selected game.
    fn open_library(&mut self, index: usize) {
        let Some(row) = self.games.get(index) else {
            return;
        };
        let (kind, title, save_dir) = (row.kind, row.title.clone(), row.save_dir.clone());
        let Some(library) = self.library.as_ref() else {
            self.stack.push(Screen::Inspect(Inspector::new(
                format!("{title} — Library"),
                vec![
                    String::new(),
                    "  No Save Library is configured.".to_string(),
                    "  Set `[library] path` in your config (see COMMANDS.md).".to_string(),
                ],
            )));
            return;
        };
        let entries = build_snapshot_items(library, kind);
        let mut list = ListState::default();
        if !entries.is_empty() {
            list.select(Some(0));
        }
        let preview = Inspector::new(
            "Preview".to_string(),
            snapshot_preview(entries.first(), save_dir.as_deref()),
        );
        self.stack.push(Screen::Library(LibraryList {
            title: format!("Library — {title}"),
            kind,
            save_dir,
            entries,
            list,
            preview,
            status: None,
            input: None,
        }));
    }

    /// Ask to confirm restoring the selected snapshot into the active save directory.
    fn request_library_restore(&mut self) {
        let (slug, has_dir) = match self.stack.last() {
            Some(Screen::Library(ll)) => {
                (ll.selected().map(|s| s.slug.clone()), ll.save_dir.is_some())
            }
            _ => (None, false),
        };
        let Some(slug) = slug else { return };
        if !has_dir {
            if let Some(Screen::Library(ll)) = self.stack.last_mut() {
                ll.status = Some("No save directory configured for this game.".to_string());
            }
            return;
        }
        self.stack.push(Screen::Confirm(Confirm {
            pending: Pending::LibraryRestore { slug },
            message: Some("Restore this snapshot over the active save?".to_string()),
        }));
    }

    /// Ask to confirm deleting the selected snapshot.
    fn request_library_delete(&mut self) {
        let target = match self.stack.last() {
            Some(Screen::Library(ll)) => ll.selected().map(|s| (s.slug.clone(), s.name.clone())),
            _ => None,
        };
        let Some((slug, name)) = target else { return };
        self.stack.push(Screen::Confirm(Confirm {
            pending: Pending::LibraryDelete { slug },
            message: Some(format!("Delete snapshot '{name}'? This cannot be undone.")),
        }));
    }

    /// Restore the named snapshot into the game's active save directory.
    fn do_library_restore(&mut self, slug: String) {
        let (kind, save_dir) = match self.stack.last() {
            Some(Screen::Library(ll)) => (ll.kind, ll.save_dir.clone()),
            _ => return,
        };
        let Some(save_dir) = save_dir else { return };
        let (status, restored) = match self.library.as_ref() {
            Some(library) => match library
                .get(kind, &slug)
                .and_then(|snap| library.restore(&snap, &save_dir))
            {
                Ok(outcome) if outcome.restored.is_empty() => (
                    "Already matches the active save; nothing restored.".to_string(),
                    Vec::new(),
                ),
                Ok(outcome) => (
                    format!(
                        "Restored ({} file(s); {} safety backup(s) made).",
                        outcome.restored.len(),
                        outcome.backups.len()
                    ),
                    outcome.restored,
                ),
                Err(e) => (format!("Restore failed: {e}"), Vec::new()),
            },
            None => ("No Save Library configured.".to_string(), Vec::new()),
        };
        for path in &restored {
            self.prune_backups(path);
        }
        // If the restored game is the one being edited, reload the session.
        if let Some(session) = self.session.as_ref() {
            if session.kind() == kind {
                if let Ok(Some(reloaded)) = Session::load(session.path()) {
                    self.session = Some(reloaded);
                }
            }
        }
        if let Some(Screen::Library(ll)) = self.stack.last_mut() {
            ll.status = Some(status);
        }
    }

    /// Delete the named snapshot and refresh the list.
    fn do_library_delete(&mut self, slug: String) {
        let kind = match self.stack.last() {
            Some(Screen::Library(ll)) => ll.kind,
            _ => return,
        };
        let status = match self.library.as_ref() {
            Some(library) => match library.delete(kind, &slug) {
                Ok(_) => format!("Deleted snapshot '{slug}'."),
                Err(e) => format!("Delete failed: {e}"),
            },
            None => "No Save Library configured.".to_string(),
        };
        let new_entries = self
            .library
            .as_ref()
            .map(|lib| build_snapshot_items(lib, kind))
            .unwrap_or_default();
        if let Some(Screen::Library(ll)) = self.stack.last_mut() {
            ll.entries = new_entries;
            ll.list.select((!ll.entries.is_empty()).then_some(0));
            refresh_library_preview(ll);
            ll.status = Some(status);
        }
    }

    /// Begin entering a name for an add / rename / duplicate operation.
    fn begin_library_input(&mut self, action: LibraryInputAction) {
        if let Some(Screen::Library(ll)) = self.stack.last_mut() {
            // Rename and duplicate need a selected snapshot.
            if action != LibraryInputAction::Add && ll.selected().is_none() {
                ll.status = Some("No snapshot selected.".to_string());
                return;
            }
            let buffer = match action {
                LibraryInputAction::Add => String::new(),
                LibraryInputAction::Rename => {
                    ll.selected().map(|s| s.name.clone()).unwrap_or_default()
                }
                LibraryInputAction::Duplicate => ll
                    .selected()
                    .map(|s| format!("{} copy", s.name))
                    .unwrap_or_default(),
            };
            ll.input = Some(LibraryInput { action, buffer });
            ll.status = None;
        }
    }

    /// Commit the name entry: add the active save, or rename / duplicate the selection.
    fn commit_library_input(&mut self, buffer: String) {
        let buffer = buffer.trim().to_string();
        let ctx = match self.stack.last() {
            Some(Screen::Library(ll)) => ll.input.as_ref().map(|inp| {
                (
                    inp.action,
                    ll.kind,
                    ll.save_dir.clone(),
                    ll.selected().map(|s| s.slug.clone()),
                )
            }),
            _ => None,
        };
        let Some((action, kind, save_dir, sel_slug)) = ctx else {
            return;
        };
        if buffer.is_empty() {
            if let Some(Screen::Library(ll)) = self.stack.last_mut() {
                ll.status = Some("Name cannot be empty.".to_string());
            }
            return;
        }

        let (status, focus_slug): (String, Option<String>) = match self.library.as_ref() {
            Some(lib) => match action {
                LibraryInputAction::Add => match &save_dir {
                    Some(dir) => match lib.add(kind, dir, &buffer, None) {
                        Ok(s) => (format!("Saved snapshot '{}'.", s.name), Some(s.slug)),
                        Err(e) => (format!("Save failed: {e}"), None),
                    },
                    None => (
                        "No save directory configured for this game.".to_string(),
                        None,
                    ),
                },
                LibraryInputAction::Rename => match &sel_slug {
                    Some(slug) => match lib.rename(kind, slug, &buffer) {
                        Ok(s) => (format!("Renamed to '{}'.", s.name), Some(s.slug)),
                        Err(e) => (format!("Rename failed: {e}"), None),
                    },
                    None => ("Nothing selected.".to_string(), None),
                },
                LibraryInputAction::Duplicate => match &sel_slug {
                    Some(slug) => match lib.duplicate(kind, slug, Some(&buffer)) {
                        Ok(s) => (format!("Duplicated as '{}'.", s.name), Some(s.slug)),
                        Err(e) => (format!("Duplicate failed: {e}"), None),
                    },
                    None => ("Nothing selected.".to_string(), None),
                },
            },
            None => ("No Save Library configured.".to_string(), None),
        };

        let new_entries = self
            .library
            .as_ref()
            .map(|lib| build_snapshot_items(lib, kind))
            .unwrap_or_default();
        if let Some(Screen::Library(ll)) = self.stack.last_mut() {
            ll.input = None;
            let idx = focus_slug
                .as_ref()
                .and_then(|slug| new_entries.iter().position(|e| &e.slug == slug))
                .unwrap_or(0);
            ll.entries = new_entries;
            ll.list.select((!ll.entries.is_empty()).then_some(idx));
            refresh_library_preview(ll);
            ll.status = Some(status);
        }
    }

    /// Open the selected game: load an editing session and show its character(s).
    /// Open the selected game: choose among its save files, or open its only one directly.
    fn open_game(&mut self, index: usize) {
        let Some(row) = self.games.get(index) else {
            return;
        };
        let title = format!("{} ({})", row.title, row.id);
        let row_title = row.title.clone();
        let inspectable = row.inspectable;
        // Copy out the file data so the `self.games` borrow ends before we mutate `self`.
        let files: Vec<(String, PathBuf, bool)> = row
            .files
            .iter()
            .map(|f| (f.name.clone(), f.path.clone(), f.found))
            .collect();

        if !inspectable {
            let msg = vec![format!("Editing {row_title} is not supported yet.")];
            self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            return;
        }
        let found: Vec<&(String, PathBuf, bool)> = files.iter().filter(|f| f.2).collect();
        match found.len() {
            0 => {
                let msg = if files.is_empty() {
                    vec!["No save directory configured for this game.".to_string()]
                } else {
                    let mut m = vec![
                        "No save files found for this game.".to_string(),
                        String::new(),
                        "Expected:".to_string(),
                    ];
                    m.extend(files.iter().map(|f| format!("  {}", f.1.display())));
                    m
                };
                self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            }
            1 => {
                let (name, path, _) = found[0].clone();
                self.open_save(path, name);
            }
            _ => {
                let entries: Vec<FileEntry> = found
                    .iter()
                    .map(|f| FileEntry {
                        label: f.0.clone(),
                        path: f.1.clone(),
                    })
                    .collect();
                let mut list = ListState::default();
                list.select(Some(0));
                self.stack.push(Screen::SaveFiles(FileList {
                    title,
                    entries,
                    list,
                }));
            }
        }
    }

    /// Open the save file selected in the file chooser.
    fn open_selected_file(&mut self, index: usize) {
        let target = match self.stack.last() {
            Some(Screen::SaveFiles(fl)) => fl
                .entries
                .get(index)
                .map(|e| (e.path.clone(), e.label.clone())),
            _ => None,
        };
        if let Some((path, label)) = target {
            self.open_save(path, label);
        }
    }

    /// Load a save file and show its character(s), or a message; pushes onto the stack.
    fn open_save(&mut self, path: PathBuf, title: String) {
        match Session::load(&path) {
            Ok(Some(session)) => {
                let entities = session.entities();
                self.session = Some(session);
                match entities.len() {
                    0 => {
                        let msg = vec!["(no characters in this save)".to_string()];
                        self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
                    }
                    1 => {
                        let entity = &entities[0];
                        self.push_editor(entity.index, entity.label.clone());
                    }
                    _ => {
                        let mut list = ListState::default();
                        list.select(Some(0));
                        self.stack.push(Screen::Characters(CharList {
                            title,
                            entities,
                            list,
                            status: None,
                        }));
                    }
                }
            }
            Ok(None) => {
                let msg = vec!["Unsupported save format.".to_string()];
                self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            }
            Err(e) => {
                let msg = vec![format!("Could not open save: {e}")];
                self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            }
        }
    }

    /// Open one character from the current character list.
    fn open_entry(&mut self, index: usize) {
        let entity = match self.stack.last() {
            Some(Screen::Characters(cl)) => {
                cl.entities.get(index).map(|e| (e.index, e.label.clone()))
            }
            _ => None,
        };
        if let Some((entity_index, label)) = entity {
            self.push_editor(entity_index, label);
        }
    }

    /// Push a field editor for the given entity, built from the current session.
    fn push_editor(&mut self, entity: usize, title: String) {
        let rows = match &self.session {
            Some(session) => session.rows(entity),
            None => return,
        };
        let display = build_display(&rows);
        let mut list = ListState::default();
        list.select(
            display
                .iter()
                .position(|d| matches!(d, DisplayRow::Field(_))),
        );
        self.stack.push(Screen::Edit(Editor {
            entity,
            title,
            rows,
            display,
            list,
            input: None,
            picker: None,
            status: None,
            edited: BTreeSet::new(),
            capture: None,
            add_item: None,
        }));
    }

    /// Commit the editor's typed value to the session buffer (validated), then refresh.
    fn commit_edit(&mut self, value: String) {
        let target = match self.stack.last() {
            Some(Screen::Edit(ed)) => ed.selected_row().map(|r| (ed.entity, r.key)),
            _ => None,
        };
        let Some((entity, key)) = target else {
            return;
        };
        let result = match &mut self.session {
            Some(session) => session.set(entity, key, &value),
            None => return,
        };
        // Re-read the entity's values so the display reflects the buffer.
        let rows = self.session.as_ref().map(|s| s.rows(entity));
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            if let Some(rows) = rows {
                ed.rows = rows;
            }
            match result {
                Ok(()) => {
                    ed.input = None;
                    ed.picker = None;
                    ed.edited.insert(key);
                    ed.status = Some(format!("Set {key} = {value}"));
                }
                Err(e) => ed.status = Some(e.to_string()), // keep input so it can be fixed
            }
        }
    }

    /// Save the current session (one backup + one write) from the editor.
    fn save_from_editor(&mut self) {
        let result = self.session.as_mut().map(|s| s.save());
        if result.as_ref().is_some_and(|r| r.is_ok()) {
            if let Some(path) = self.session.as_ref().map(|s| s.path().to_path_buf()) {
                self.prune_backups(&path);
            }
        }
        // The session is the whole save file, so a save can be triggered from the character
        // list or from inside a character; show the result on whichever screen we're on.
        let status = match result {
            Some(Ok(backup)) => format!("Saved. Backup: {}", backup.display()),
            Some(Err(e)) => format!("Save failed: {e}"),
            None => "Nothing to save.".to_string(),
        };
        match self.stack.last_mut() {
            Some(Screen::Edit(ed)) => ed.status = Some(status),
            Some(Screen::Characters(cl)) => cl.status = Some(status),
            _ => {}
        }
    }

    /// Prune old automatic backups of `path` per the configured retention policy.
    fn prune_backups(&self, path: &Path) {
        let _ = backup::prune(path, &self.retention);
    }

    /// Open the "add item" picker for the current character (games with an item list only).
    fn open_add_item(&mut self) {
        let catalog = match &self.session {
            Some(s) if s.supports_items() => s.item_catalog(),
            _ => return,
        };
        if catalog.is_empty() {
            return;
        }
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            if ed.entity == 0 {
                return; // the party entity has no item list
            }
            ed.add_item = Some(ItemAdd::new(catalog));
        }
    }

    /// Append the chosen item (unloaded) to the current character, then refresh the editor.
    fn commit_add_item(&mut self, id: u8) {
        let entity = match self.stack.last() {
            Some(Screen::Edit(ed)) => ed.entity,
            _ => return,
        };
        let result = self.session.as_mut().map(|s| s.add_item(entity, id, 0));
        let rows = self.session.as_ref().map(|s| s.rows(entity));
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            ed.add_item = None;
            if let Some(rows) = rows {
                ed.rows = rows;
                ed.display = build_display(&ed.rows);
            }
            ed.status = Some(match result {
                Some(Ok(slot)) => {
                    format!("Added item (slot {slot}). Set its ammo below, then press s to save.")
                }
                Some(Err(e)) => format!("Add failed: {e}"),
                None => "Nothing to add.".to_string(),
            });
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        let games_len = self.games.len();
        let mut action = Action::None;
        match self.stack.last_mut().expect("stack is never empty") {
            Screen::Games(list) => match code {
                KeyCode::Char('q') => action = Action::Quit,
                KeyCode::Esc => action = Action::Back,
                KeyCode::Down | KeyCode::Char('j') => select_wrap(list, games_len, 1),
                KeyCode::Up | KeyCode::Char('k') => select_wrap(list, games_len, -1),
                KeyCode::Char('r') => action = Action::OpenResources(list.selected()),
                KeyCode::Char('L') => action = Action::OpenLibrary(list.selected()),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    action = Action::OpenGame(list.selected());
                }
                _ => {}
            },
            Screen::SaveFiles(fl) => {
                let len = fl.entries.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => select_wrap(&mut fl.list, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => select_wrap(&mut fl.list, len, -1),
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        action = Action::OpenSaveFile(fl.list.selected());
                    }
                    _ => {}
                }
            }
            Screen::Characters(cl) => {
                let len = cl.entities.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cl.status = None;
                        select_wrap(&mut cl.list, len, 1);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        cl.status = None;
                        select_wrap(&mut cl.list, len, -1);
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        action = Action::OpenEntry(cl.list.selected());
                    }
                    // Backups snapshot the whole save file, so they're reachable here without
                    // first entering a character.
                    KeyCode::Char('b') => action = Action::OpenBackups,
                    // The whole file saves at once, so Save is offered at this top level too
                    // (not only from inside a character).
                    KeyCode::Char('s') => action = Action::Save,
                    _ => {}
                }
            }
            Screen::Edit(ed) => {
                if let Some(buf) = &mut ed.input {
                    match code {
                        KeyCode::Enter => action = Action::Commit(buf.clone()),
                        KeyCode::Esc => {
                            ed.input = None;
                            ed.status = None;
                        }
                        KeyCode::Backspace => {
                            buf.pop();
                        }
                        KeyCode::Char(c) => buf.push(c),
                        _ => {}
                    }
                } else if let Some(p) = &mut ed.picker {
                    let len = p.options.len();
                    match code {
                        KeyCode::Left | KeyCode::Up | KeyCode::Char('h') | KeyCode::Char('k') => {
                            if len > 0 {
                                p.index = (p.index + len - 1) % len;
                            }
                        }
                        KeyCode::Right
                        | KeyCode::Down
                        | KeyCode::Char('l')
                        | KeyCode::Char('j') => {
                            if len > 0 {
                                p.index = (p.index + 1) % len;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(value) = p.options.get(p.index).cloned() {
                                action = Action::Commit(value);
                            }
                        }
                        KeyCode::Esc => {
                            ed.picker = None;
                            ed.status = None;
                        }
                        _ => {}
                    }
                } else if ed.capture.is_some() {
                    handle_capture_key(ed, code, &mut action);
                } else if let Some(ia) = &mut ed.add_item {
                    // Type to filter the item list; arrows navigate; Enter adds, Esc cancels.
                    let len = ia.matches.len();
                    match code {
                        KeyCode::Up => select_wrap(&mut ia.list, len, -1),
                        KeyCode::Down => select_wrap(&mut ia.list, len, 1),
                        KeyCode::Enter => {
                            if let Some((id, _)) = ia.selected() {
                                action = Action::AddItemCommit(id);
                            }
                        }
                        KeyCode::Esc => {
                            ed.add_item = None;
                            ed.status = None;
                        }
                        KeyCode::Backspace => {
                            ia.filter.pop();
                            ia.refilter();
                        }
                        KeyCode::Char(c) => {
                            ia.filter.push(c);
                            ia.refilter();
                        }
                        _ => {}
                    }
                } else {
                    match code {
                        KeyCode::Char('q') => action = Action::Quit,
                        KeyCode::Esc | KeyCode::Left => action = Action::Back,
                        KeyCode::Down | KeyCode::Char('j') => ed.move_selection(1),
                        KeyCode::Up | KeyCode::Char('k') => ed.move_selection(-1),
                        KeyCode::Enter | KeyCode::Char('e') => ed.begin_edit(),
                        KeyCode::Char('s') => action = Action::Save,
                        KeyCode::Char('a') => action = Action::AddItem,
                        KeyCode::Char('b') => action = Action::OpenBackups,
                        KeyCode::Char('t') => action = Action::OpenTemplates,
                        KeyCode::Char('T') => ed.begin_capture(),
                        _ => {}
                    }
                }
            }
            Screen::Backups(bl) => {
                let len = bl.entries.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        select_wrap(&mut bl.list, len, 1);
                        bl.status = None;
                        refresh_backup_preview(bl);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        select_wrap(&mut bl.list, len, -1);
                        bl.status = None;
                        refresh_backup_preview(bl);
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => bl.preview.page_down(),
                    KeyCode::PageUp => bl.preview.page_up(),
                    KeyCode::Home => bl.preview.home(),
                    KeyCode::End => bl.preview.end(),
                    KeyCode::Enter | KeyCode::Char('r') => action = Action::RequestRestore,
                    KeyCode::Char('n') => action = Action::Snapshot,
                    _ => {}
                }
            }
            Screen::Templates(tl) => {
                let len = tl.entries.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        select_wrap(&mut tl.list, len, 1);
                        tl.status = None;
                        refresh_template_preview(tl);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        select_wrap(&mut tl.list, len, -1);
                        tl.status = None;
                        refresh_template_preview(tl);
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => tl.preview.page_down(),
                    KeyCode::PageUp => tl.preview.page_up(),
                    KeyCode::Home => tl.preview.home(),
                    KeyCode::End => tl.preview.end(),
                    KeyCode::Enter | KeyCode::Char('a') => action = Action::ApplyTemplate,
                    _ => {}
                }
            }
            Screen::Inspect(insp) => match code {
                KeyCode::Char('q') => action = Action::Quit,
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                    action = Action::Back;
                }
                KeyCode::Down | KeyCode::Char('j') => insp.scroll_down(),
                KeyCode::Up | KeyCode::Char('k') => insp.scroll_up(),
                KeyCode::PageDown | KeyCode::Char(' ') => insp.page_down(),
                KeyCode::PageUp => insp.page_up(),
                KeyCode::Home | KeyCode::Char('g') => insp.home(),
                KeyCode::End | KeyCode::Char('G') => insp.end(),
                _ => {}
            },
            Screen::Resources(rl) => {
                let len = rl.entries.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        select_wrap(&mut rl.list, len, 1);
                        rl.status = None;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        select_wrap(&mut rl.list, len, -1);
                        rl.status = None;
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('o') => {
                        action = Action::OpenResourceLink(rl.list.selected());
                    }
                    _ => {}
                }
            }
            Screen::Library(ll) => {
                if let Some(inp) = &mut ll.input {
                    match code {
                        KeyCode::Enter => action = Action::LibraryCommitInput(inp.buffer.clone()),
                        KeyCode::Esc => {
                            ll.input = None;
                            ll.status = None;
                        }
                        KeyCode::Backspace => {
                            inp.buffer.pop();
                        }
                        KeyCode::Char(c) => inp.buffer.push(c),
                        _ => {}
                    }
                } else {
                    let len = ll.entries.len();
                    match code {
                        KeyCode::Char('q') => action = Action::Quit,
                        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                            action = Action::Back;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            select_wrap(&mut ll.list, len, 1);
                            ll.status = None;
                            refresh_library_preview(ll);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            select_wrap(&mut ll.list, len, -1);
                            ll.status = None;
                            refresh_library_preview(ll);
                        }
                        KeyCode::PageDown | KeyCode::Char(' ') => ll.preview.page_down(),
                        KeyCode::PageUp => ll.preview.page_up(),
                        KeyCode::Enter | KeyCode::Char('r') => {
                            action = Action::LibraryRestoreRequest;
                        }
                        KeyCode::Char('d') => action = Action::LibraryDeleteRequest,
                        KeyCode::Char('a') => {
                            action = Action::LibraryBeginInput(LibraryInputAction::Add);
                        }
                        KeyCode::Char('R') => {
                            action = Action::LibraryBeginInput(LibraryInputAction::Rename);
                        }
                        KeyCode::Char('D') => {
                            action = Action::LibraryBeginInput(LibraryInputAction::Duplicate);
                        }
                        _ => {}
                    }
                }
            }
            Screen::Confirm(c) => match &c.pending {
                Pending::Restore { .. }
                | Pending::LibraryRestore { .. }
                | Pending::LibraryDelete { .. } => match code {
                    KeyCode::Char('y') | KeyCode::Enter => action = Action::ConfirmAccept,
                    KeyCode::Esc | KeyCode::Char('n') => action = Action::ConfirmCancel,
                    _ => {}
                },
                _ => match code {
                    KeyCode::Char('s') => action = Action::ConfirmSave,
                    KeyCode::Char('d') => action = Action::ConfirmDiscard,
                    KeyCode::Esc | KeyCode::Char('n') => action = Action::ConfirmCancel,
                    _ => {}
                },
            },
        }
        match action {
            Action::None => {}
            Action::Back => self.back(),
            Action::Quit => self.request_quit(),
            Action::OpenGame(Some(i)) => self.open_game(i),
            Action::OpenSaveFile(Some(i)) => self.open_selected_file(i),
            Action::OpenEntry(Some(i)) => self.open_entry(i),
            Action::Commit(v) => self.commit_edit(v),
            Action::Save => self.save_from_editor(),
            Action::AddItem => self.open_add_item(),
            Action::AddItemCommit(id) => self.commit_add_item(id),
            Action::OpenBackups => self.open_backups(),
            Action::RequestRestore => self.request_restore(),
            Action::Snapshot => self.snapshot_current(),
            Action::OpenTemplates => self.open_templates(),
            Action::ApplyTemplate => self.apply_template_selected(),
            Action::SaveTemplate(name) => self.save_template(name),
            Action::OpenResources(Some(i)) => self.open_resources(i),
            Action::OpenResourceLink(Some(i)) => self.open_resource_link(i),
            Action::OpenLibrary(Some(i)) => self.open_library(i),
            Action::LibraryRestoreRequest => self.request_library_restore(),
            Action::LibraryDeleteRequest => self.request_library_delete(),
            Action::LibraryBeginInput(a) => self.begin_library_input(a),
            Action::LibraryCommitInput(name) => self.commit_library_input(name),
            Action::ConfirmSave => self.confirm_save(),
            Action::ConfirmDiscard => self.confirm_discard(),
            Action::ConfirmAccept => self.complete_pending(),
            Action::ConfirmCancel => self.confirm_cancel(),
            Action::OpenGame(None) | Action::OpenSaveFile(None) | Action::OpenEntry(None) => {}
            Action::OpenResources(None) | Action::OpenResourceLink(None) => {}
            Action::OpenLibrary(None) => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());

        let dirty = self.session_dirty();
        let bottom = bottom_line(self.stack.last().expect("stack is never empty"));
        let games = &self.games;
        match self.stack.last_mut().expect("stack is never empty") {
            Screen::Games(list) => draw_games(frame, chunks[0], games, list),
            Screen::SaveFiles(fl) => draw_savefiles(frame, chunks[0], fl),
            Screen::Characters(cl) => draw_characters(frame, chunks[0], cl),
            Screen::Edit(ed) => draw_editor(frame, chunks[0], ed, dirty),
            Screen::Backups(bl) => draw_backups(frame, chunks[0], bl),
            Screen::Templates(tl) => draw_templates(frame, chunks[0], tl),
            Screen::Resources(rl) => draw_resources(frame, chunks[0], rl),
            Screen::Library(ll) => draw_library(frame, chunks[0], ll),
            Screen::Inspect(insp) => {
                insp.viewport = chunks[0].height.saturating_sub(2);
                draw_inspector(frame, chunks[0], insp);
            }
            Screen::Confirm(c) => draw_confirm(frame, chunks[0], c),
        }

        // The enum picker gets a styled option strip; every other screen uses plain text.
        let picker_line = match self.stack.last().expect("stack is never empty") {
            Screen::Edit(ed) => picker_bottom_line(ed),
            _ => None,
        };
        match picker_line {
            Some(line) => frame.render_widget(Paragraph::new(line), chunks[1]),
            None => frame.render_widget(
                Paragraph::new(bottom).style(Style::default().fg(Color::DarkGray)),
                chunks[1],
            ),
        }
    }
}

/// A styled option strip for the enum picker: the full list of values with the current one
/// highlighted, so the user can see everything they're cycling through. It's rendered on a
/// single line, so a long list is simply clipped on narrow terminals (the active editor row
/// still shows the current value).
fn picker_bottom_line(ed: &Editor) -> Option<Line<'static>> {
    let p = ed.picker.as_ref()?;
    let label = ed.selected_row().map(|r| r.label).unwrap_or("value");
    let mut spans = vec![Span::styled(
        format!(" {label}:  "),
        Style::default().fg(Color::DarkGray),
    )];
    for (i, opt) in p.options.iter().enumerate() {
        let style = if i == p.index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(format!(" {opt} "), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        " (←/→ Enter Esc)",
        Style::default().fg(Color::DarkGray),
    ));
    Some(Line::from(spans))
}

/// Move a list selection by `delta`, wrapping around a list of `len` items.
fn select_wrap(list: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let current = list.selected().unwrap_or(0) as i32;
    let next = (current + delta).rem_euclid(len as i32) as usize;
    list.select(Some(next));
}

/// Handle a keypress while the editor is in template-capture mode (selecting fields, then
/// naming the template).
fn handle_capture_key(ed: &mut Editor, code: KeyCode, action: &mut Action) {
    let naming = ed.capture.as_ref().is_some_and(|c| c.naming.is_some());
    if naming {
        let submit = {
            let buf = ed
                .capture
                .as_mut()
                .and_then(|c| c.naming.as_mut())
                .expect("naming buffer");
            match code {
                KeyCode::Enter => (!buf.trim().is_empty()).then(|| buf.trim().to_string()),
                KeyCode::Backspace => {
                    buf.pop();
                    None
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    None
                }
                _ => None,
            }
        };
        if let Some(name) = submit {
            *action = Action::SaveTemplate(name);
        } else if code == KeyCode::Esc {
            if let Some(c) = ed.capture.as_mut() {
                c.naming = None; // back to field selection
            }
        }
        return;
    }

    match code {
        KeyCode::Esc => {
            ed.capture = None;
            ed.status = None;
        }
        KeyCode::Down | KeyCode::Char('j') => ed.move_selection(1),
        KeyCode::Up | KeyCode::Char('k') => ed.move_selection(-1),
        KeyCode::Char(' ') => {
            let field = ed.selected_field();
            if let (Some(fi), Some(c)) = (field, ed.capture.as_mut()) {
                if let Some(flag) = c.selected.get_mut(fi) {
                    *flag = !*flag;
                }
            }
        }
        KeyCode::Char('a') => {
            if let Some(c) = ed.capture.as_mut() {
                let all = c.selected.iter().all(|s| *s);
                c.selected.iter_mut().for_each(|s| *s = !all);
            }
        }
        KeyCode::Enter => {
            let any = ed
                .capture
                .as_ref()
                .is_some_and(|c| c.selected.iter().any(|s| *s));
            if any {
                if let Some(c) = ed.capture.as_mut() {
                    c.naming = Some(String::new());
                }
            } else {
                ed.status = Some("Select at least one field (Space).".to_string());
            }
        }
        _ => {}
    }
}

fn selectable_list<'a>(items: Vec<ListItem<'a>>, title: String) -> List<'a> {
    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ")
}

fn draw_games(frame: &mut Frame, area: Rect, games: &[GameRow], list: &mut ListState) {
    let items: Vec<ListItem> = if games.is_empty() {
        vec![ListItem::new(
            "No games configured. See config.example.toml.",
        )]
    } else {
        games
            .iter()
            .map(|g| {
                let status = if !g.inspectable {
                    "not supported"
                } else if g.found() {
                    "found"
                } else {
                    "missing"
                };
                ListItem::new(Line::from(format!(
                    "{:<12} {:<12} [{}]",
                    g.id, g.title, status
                )))
            })
            .collect()
    };
    let widget = selectable_list(items, " Fringe Retro Kit — Games ".to_string());
    frame.render_stateful_widget(widget, area, list);
}

fn draw_savefiles(frame: &mut Frame, area: Rect, fl: &mut FileList) {
    let items: Vec<ListItem> = fl
        .entries
        .iter()
        .map(|e| ListItem::new(Line::from(e.label.clone())))
        .collect();
    let widget = selectable_list(items, format!(" {} — choose a save file ", fl.title));
    frame.render_stateful_widget(widget, area, &mut fl.list);
}

fn draw_characters(frame: &mut Frame, area: Rect, cl: &mut CharList) {
    let items: Vec<ListItem> = cl
        .entities
        .iter()
        .map(|e| ListItem::new(Line::from(e.label.clone())))
        .collect();
    let widget = selectable_list(items, format!(" {} ", cl.title));
    frame.render_stateful_widget(widget, area, &mut cl.list);
}

fn draw_resources(frame: &mut Frame, area: Rect, rl: &mut ResourceList) {
    let items: Vec<ListItem> = rl
        .entries
        .iter()
        .map(|r| {
            let heading = Line::from(vec![
                Span::styled(
                    format!("[{}] ", r.category),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(r.title.clone()),
            ]);
            let url = Line::from(Span::styled(
                format!("    {}", r.url),
                Style::default().fg(Color::DarkGray),
            ));
            ListItem::new(vec![heading, url])
        })
        .collect();
    let widget = selectable_list(items, format!(" {} — Resources ", rl.game_title));
    frame.render_stateful_widget(widget, area, &mut rl.list);
}

/// Build the snapshot rows for a game's Save Library.
fn build_snapshot_items(library: &Library, kind: GameKind) -> Vec<SnapshotItem> {
    library
        .list(Some(kind))
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            let updated = s.last_updated.map(|t| {
                chrono::DateTime::<chrono::Local>::from(t)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            });
            let label = match &updated {
                Some(u) => format!("{}  ({u})", s.name),
                None => s.name.clone(),
            };
            SnapshotItem {
                name: s.name,
                slug: s.slug,
                notes: s.notes,
                updated,
                label,
                dir: s.dir,
                files: s.files,
            }
        })
        .collect()
}

/// A decoded preview of a snapshot: its metadata plus each save file's inspection.
/// Preview a Library snapshot: a per-file diff against the current save ("what changed since
/// this snapshot"), followed by each file's full contents so absolute values stay visible.
fn snapshot_preview(item: Option<&SnapshotItem>, save_dir: Option<&Path>) -> Vec<String> {
    let Some(item) = item else {
        return vec!["(no snapshots yet — press 'a' to add one)".to_string()];
    };
    let mut lines = vec![format!("{}  [{}]", item.name, item.slug)];
    if let Some(updated) = &item.updated {
        lines.push(format!("updated: {updated}"));
    }
    if let Some(notes) = &item.notes {
        lines.push(format!("notes: {notes}"));
    }

    // Diff each snapshot file against the current save (snapshot -> current).
    if let Some(save_dir) = save_dir {
        lines.push(String::new());
        lines.push("Changes since this snapshot:".to_string());
        for file in &item.files {
            lines.push(format!("{file}:"));
            let current = save_dir.join(file);
            if current.exists() {
                match crate::compare::compare(&item.dir.join(file), &current) {
                    Ok(comparison) => lines.extend(
                        crate::compare::report(&comparison)
                            .into_iter()
                            .map(|l| format!("  {l}")),
                    ),
                    Err(e) => lines.push(format!("  (cannot diff: {e})")),
                }
            } else {
                lines.push("  (not in the current save)".to_string());
            }
        }
    }

    // Each file's full contents.
    lines.push(String::new());
    lines.push("── Snapshot contents ──".to_string());
    for file in &item.files {
        lines.push(String::new());
        lines.push(format!("{file}:"));
        match std::fs::read(item.dir.join(file)) {
            Ok(bytes) => match crate::inspect::inspect_lines(&bytes) {
                Ok(inspected) => lines.extend(inspected.into_iter().map(|l| format!("  {l}"))),
                Err(e) => lines.push(format!("  (cannot preview: {e})")),
            },
            Err(e) => lines.push(format!("  (cannot read: {e})")),
        }
    }
    lines
}

/// Rebuild the preview pane for whichever snapshot is currently selected.
fn refresh_library_preview(ll: &mut LibraryList) {
    ll.preview = Inspector::new(
        "Preview".to_string(),
        snapshot_preview(ll.selected(), ll.save_dir.as_deref()),
    );
}

fn draw_library(frame: &mut Frame, area: Rect, ll: &mut LibraryList) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(46), Constraint::Min(1)])
        .split(area);

    let items: Vec<ListItem> = if ll.entries.is_empty() {
        vec![ListItem::new("(no snapshots yet — press 'a' to add)")]
    } else {
        ll.entries
            .iter()
            .map(|e| ListItem::new(Line::from(e.label.clone())))
            .collect()
    };
    let list = selectable_list(items, format!(" {} ", ll.title));
    frame.render_stateful_widget(list, cols[0], &mut ll.list);

    ll.preview.viewport = cols[1].height.saturating_sub(2);
    draw_inspector(frame, cols[1], &ll.preview);
}

fn draw_editor(frame: &mut Frame, area: Rect, ed: &mut Editor, dirty: bool) {
    // While adding an item, the list area shows a type-to-filter item picker instead of fields.
    if let Some(ia) = &mut ed.add_item {
        let items: Vec<ListItem> = ia
            .matches
            .iter()
            .map(|&mi| {
                let (id, name) = ia.catalog[mi];
                ListItem::new(Line::from(format!("  {name}  (id {id})")))
            })
            .collect();
        let widget = selectable_list(items, format!(" Add item — filter: {}_ ", ia.filter));
        frame.render_stateful_widget(widget, area, &mut ia.list);
        return;
    }
    let capture = ed.capture.as_ref().map(|c| &c.selected);
    let picker = ed.picker.as_ref();
    let cursor = ed.list.selected();
    let items: Vec<ListItem> = ed
        .display
        .iter()
        .enumerate()
        .map(|(di, d)| match d {
            DisplayRow::Header(section) => ListItem::new(Line::from(Span::styled(
                section.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))),
            DisplayRow::Field(fi) => {
                let r = &ed.rows[*fi];
                let text = match capture {
                    Some(flags) => {
                        let mark = if *flags.get(*fi).unwrap_or(&false) {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        format!("{mark} {:<16} {}", r.label, r.value)
                    }
                    None => {
                        // While picking, show the pending value on the active row.
                        let value = match (picker, cursor) {
                            (Some(p), Some(c)) if c == di => {
                                p.options.get(p.index).cloned().unwrap_or_default()
                            }
                            _ => r.value.clone(),
                        };
                        format!("  {:<16} {}", r.label, value)
                    }
                };
                ListItem::new(Line::from(text))
            }
        })
        .collect();
    let title = if ed.capture.is_some() {
        format!(" Capture template — {} ", ed.title)
    } else {
        let marker = if dirty { "● " } else { "" };
        format!(" {marker}{} ", ed.title)
    };
    let widget = selectable_list(items, title);
    frame.render_stateful_widget(widget, area, &mut ed.list);
}

fn draw_confirm(frame: &mut Frame, area: Rect, c: &Confirm) {
    let (default_msg, choices, title) = match &c.pending {
        Pending::Restore { .. } => (
            "Restore this backup over the current save?",
            "[y] restore\n[Esc] cancel",
            " Confirm restore ",
        ),
        _ => (
            "You have unsaved changes.",
            "[s] save and continue\n[d] discard changes\n[Esc] cancel",
            " Unsaved changes ",
        ),
    };
    let msg = c.message.as_deref().unwrap_or(default_msg);
    let text = format!("{msg}\n\n{choices}");
    let widget = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(widget, area);
}

/// A decoded, human-readable preview of a backup file (or a friendly error line).
/// Preview a backup: a field-level diff against the current save ("what changed since this
/// backup"), followed by the backup's full contents so absolute values are always visible.
fn backup_preview(backup: &Path, save: &Path) -> Vec<String> {
    let mut out = vec!["Changes since this backup:".to_string()];
    match crate::compare::compare(backup, save) {
        Ok(comparison) => out.extend(crate::compare::report(&comparison)),
        Err(e) => out.push(format!("(cannot diff: {e})")),
    }
    out.push(String::new());
    out.push("── Backup contents ──".to_string());
    out.push(String::new());
    match std::fs::read(backup) {
        Ok(bytes) => out.extend(
            crate::inspect::inspect_lines(&bytes)
                .unwrap_or_else(|e| vec![format!("(cannot preview: {e})")]),
        ),
        Err(e) => out.push(format!("(cannot read: {e})")),
    }
    out
}

/// The timestamp portion of a backup file name (strips the `<save>.` prefix and `.bak`).
fn backup_stamp(backup: &Path, target: &Path) -> String {
    let name = backup
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = target
        .file_name()
        .map(|n| format!("{}.", n.to_string_lossy()))
        .unwrap_or_default();
    let stamp = name.strip_prefix(&prefix).unwrap_or(&name);
    stamp.strip_suffix(".bak").unwrap_or(stamp).to_string()
}

/// List the backups for `path`, newest first, labelled with timestamp/size and a marker on
/// any backup that is byte-identical to the current save.
fn build_backup_entries(path: &Path) -> Vec<BackupEntry> {
    let current = std::fs::read(path).ok();
    backup::list(path)
        .unwrap_or_default()
        .iter()
        .rev()
        .map(|b| {
            let size = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
            let is_current = current
                .as_ref()
                .is_some_and(|c| std::fs::read(b).is_ok_and(|bb| &bb == c));
            let marker = if is_current { "  ← current" } else { "" };
            BackupEntry {
                label: format!("{}  {size} bytes{marker}", backup_stamp(b, path)),
                is_current,
                path: b.clone(),
            }
        })
        .collect()
}

/// Rebuild the preview pane for whichever backup is currently selected.
fn refresh_backup_preview(bl: &mut BackupList) {
    let content = match bl.list.selected().and_then(|i| bl.entries.get(i)) {
        Some(e) => backup_preview(&e.path, &bl.save_path),
        None => vec!["(no backups yet)".to_string()],
    };
    bl.preview = Inspector::new("Preview".to_string(), content);
}

fn draw_backups(frame: &mut Frame, area: Rect, bl: &mut BackupList) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(50), Constraint::Min(1)])
        .split(area);

    let items: Vec<ListItem> = if bl.entries.is_empty() {
        vec![ListItem::new("(no backups yet)")]
    } else {
        bl.entries
            .iter()
            .map(|e| {
                let style = if e.is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(e.label.clone())).style(style)
            })
            .collect()
    };
    let list = selectable_list(items, format!(" {} ", bl.title));
    frame.render_stateful_widget(list, cols[0], &mut bl.list);

    bl.preview.viewport = cols[1].height.saturating_sub(2);
    draw_inspector(frame, cols[1], &bl.preview);
}

/// A preview of a template: its description, the field/value pairs it sets, and any error.
fn template_preview(item: &TemplateItem) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(d) = &item.description {
        lines.push(d.clone());
        lines.push(String::new());
    }
    if item.fields.is_empty() {
        lines.push("(no fields)".to_string());
    } else {
        for (k, v) in &item.fields {
            lines.push(format!("  {k:<18} = {v}"));
        }
    }
    if let Some(err) = &item.error {
        lines.push(String::new());
        lines.push(format!("⚠ invalid: {err}"));
    }
    lines
}

/// Rebuild the preview pane for whichever template is currently selected.
fn refresh_template_preview(tl: &mut TemplateList) {
    let content = match tl.list.selected().and_then(|i| tl.entries.get(i)) {
        Some(e) => template_preview(e),
        None => vec!["(no templates)".to_string()],
    };
    tl.preview = Inspector::new("Preview".to_string(), content);
}

fn draw_templates(frame: &mut Frame, area: Rect, tl: &mut TemplateList) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(1)])
        .split(area);

    let items: Vec<ListItem> = tl
        .entries
        .iter()
        .map(|e| {
            let (label, style) = match &e.error {
                Some(_) => (format!("{}  ✗", e.name), Style::default().fg(Color::Red)),
                None => (e.name.clone(), Style::default()),
            };
            ListItem::new(Line::from(label)).style(style)
        })
        .collect();
    let list = selectable_list(items, format!(" {} ", tl.title));
    frame.render_stateful_widget(list, cols[0], &mut tl.list);

    tl.preview.viewport = cols[1].height.saturating_sub(2);
    draw_inspector(frame, cols[1], &tl.preview);
}

/// The context-dependent bottom line: key hints, the field input prompt, or a status.
fn bottom_line(screen: &Screen) -> String {
    match screen {
        Screen::Games(_) => " ↑/↓ select · Enter open · r resources · L library · q quit ".to_string(),
        Screen::SaveFiles(_) => {
            " ↑/↓ select · Enter open · Esc back · q quit ".to_string()
        }
        Screen::Characters(cl) => match &cl.status {
            Some(s) => format!("  {s}"),
            None => " ↑/↓ select · Enter open · s save · b backups · Esc back · q quit ".to_string(),
        },
        Screen::Edit(ed) => {
            if let Some(input) = &ed.input {
                let label = ed.selected_row().map(|r| r.label).unwrap_or("value");
                match &ed.status {
                    Some(s) => format!("  {label}: {input}_   {s}"),
                    None => format!("  {label}: {input}_   (Enter commit · Esc cancel)"),
                }
            } else if let Some(ia) = &ed.add_item {
                format!(
                    "  Add item: {}_   (type to filter · ↑/↓ · Enter add · Esc cancel · {} matches)",
                    ia.filter,
                    ia.matches.len()
                )
            } else if let Some(p) = &ed.picker {
                let label = ed.selected_row().map(|r| r.label).unwrap_or("value");
                let value = p.options.get(p.index).map(|s| s.as_str()).unwrap_or("");
                format!("  {label}: ◄ {value} ►   (←/→ choose · Enter set · Esc cancel)")
            } else if let Some(cap) = &ed.capture {
                if let Some(buf) = &cap.naming {
                    format!("  Template name: {buf}_   (Enter save · Esc back)")
                } else {
                    let n = cap.selected.iter().filter(|s| **s).count();
                    match &ed.status {
                        Some(s) => format!("  {s}"),
                        None => {
                            format!(" Space toggle · a all · Enter name · Esc cancel · {n} selected ")
                        }
                    }
                }
            } else if let Some(status) = &ed.status {
                format!("  {status}")
            } else {
                " ↑/↓ field · Enter/e edit · s save · a add item · b backups · t templates · T capture · Esc back · q quit ".to_string()
            }
        }
        Screen::Inspect(_) => " ↑/↓ scroll · PgUp/PgDn page · Esc back · q quit ".to_string(),
        Screen::Backups(bl) => match &bl.status {
            Some(s) => format!("  {s}"),
            None => {
                " ↑/↓ select · PgUp/PgDn preview · Enter/r restore · n snapshot · Esc back · q quit "
                    .to_string()
            }
        },
        Screen::Templates(tl) => match &tl.status {
            Some(s) => format!("  {s}"),
            None => {
                " ↑/↓ select · PgUp/PgDn preview · Enter/a apply · Esc back · q quit ".to_string()
            }
        },
        Screen::Resources(rl) => match &rl.status {
            Some(s) => format!("  {s}"),
            None => " ↑/↓ select · Enter open in browser · Esc back · q quit ".to_string(),
        },
        Screen::Library(ll) => {
            if let Some(inp) = &ll.input {
                let label = match inp.action {
                    LibraryInputAction::Add => "New snapshot name",
                    LibraryInputAction::Rename => "Rename to",
                    LibraryInputAction::Duplicate => "Duplicate as",
                };
                format!("  {label}: {}_   (Enter save · Esc cancel)", inp.buffer)
            } else if let Some(s) = &ll.status {
                format!("  {s}")
            } else {
                " ↑/↓ select · Enter/r restore · a add · R rename · D duplicate · d delete · Esc back · q quit "
                    .to_string()
            }
        }
        Screen::Confirm(c) => match &c.pending {
            Pending::Restore { .. } => " y restore · Esc cancel ".to_string(),
            _ => " s save · d discard · Esc cancel ".to_string(),
        },
    }
}

fn draw_inspector(frame: &mut Frame, area: Rect, insp: &Inspector) {
    let paragraph = Paragraph::new(insp.content.join("\n"))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", insp.title)),
        )
        .wrap(Wrap { trim: false })
        .scroll((insp.scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Run the interactive browser. Sets up and restores the terminal, and blocks until quit.
pub fn run(config: Config) -> Result<()> {
    let mut games = Vec::new();
    for g in config.games()? {
        let files = match &g.save_dir {
            Some(dir) => g
                .kind
                .save_files()
                .iter()
                .map(|name| {
                    let path = dir.join(name);
                    let found = path.exists();
                    SaveFile {
                        name: name.to_string(),
                        path,
                        found,
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        games.push(GameRow {
            id: g.id,
            title: g.kind.title().to_string(),
            kind: g.kind,
            inspectable: g.kind.is_inspectable(),
            save_dir: g.save_dir.clone(),
            files,
        });
    }

    let mut app = App::new(games);
    match TemplateSet::load() {
        Ok(set) => app.templates = set,
        Err(e) => app.template_error = Some(e.to_string()),
    }
    // Merge any user resource overrides; on a bad user file, keep the bundled defaults.
    app.resources = Resources::load().unwrap_or_else(|_| Resources::bundled());
    app.library = config.library().ok();
    app.retention = config.retention();
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, app);
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while app.running {
        terminal.draw(|frame| app.draw(frame))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key.code);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(n: usize) -> App {
        let games = (0..n)
            .map(|i| GameRow {
                id: format!("g{i}"),
                title: format!("Game {i}"),
                kind: GameKind::Ultima1,
                inspectable: true,
                save_dir: None,
                files: Vec::new(),
            })
            .collect();
        App::new(games)
    }

    fn games_selection(app: &App) -> Option<usize> {
        match app.stack.last() {
            Some(Screen::Games(list)) => list.selected(),
            _ => None,
        }
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut app = app_with(3);
        assert_eq!(games_selection(&app), Some(0));
        app.handle_key(KeyCode::Down);
        assert_eq!(games_selection(&app), Some(1));
        app.handle_key(KeyCode::Up);
        app.handle_key(KeyCode::Up);
        assert_eq!(games_selection(&app), Some(2)); // wrapped past 0
        app.handle_key(KeyCode::Down);
        assert_eq!(games_selection(&app), Some(0)); // wrapped past end
    }

    #[test]
    fn empty_list_navigation_is_safe() {
        let mut app = app_with(0);
        assert_eq!(games_selection(&app), None);
        app.handle_key(KeyCode::Down); // must not panic
        assert_eq!(games_selection(&app), None);
    }

    #[test]
    fn esc_at_top_level_quits() {
        let mut app = app_with(1);
        app.handle_key(KeyCode::Esc);
        assert!(!app.running);
    }

    #[test]
    fn open_single_game_shows_inspector_and_esc_returns() {
        // A not-found game yields a Single browse, so Enter goes straight to the inspector.
        let mut app = app_with(1);
        app.handle_key(KeyCode::Enter);
        assert!(matches!(app.stack.last(), Some(Screen::Inspect(_))));
        app.handle_key(KeyCode::Esc);
        assert!(matches!(app.stack.last(), Some(Screen::Games(_))));
    }

    /// Build an app with one Ultima I game backed by a real temp save file.
    fn app_with_ultima1() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        let mut bytes = vec![0u8; fringe_retro_core::games::ultima1::SAVE_LEN];
        bytes[0..4].copy_from_slice(b"Enki");
        std::fs::write(&path, &bytes).unwrap();
        let app = App::new(vec![GameRow {
            id: "u1".to_string(),
            title: "Ultima I".to_string(),
            kind: GameKind::Ultima1,
            inspectable: true,
            save_dir: Some(dir.path().to_path_buf()),
            files: vec![SaveFile {
                name: "PLAYER1.U1".to_string(),
                path,
                found: true,
            }],
        }]);
        (dir, app)
    }

    fn app_with_ultima3_party() -> (tempfile::TempDir, App) {
        use fringe_retro_core::games::ultima3;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PARTY.ULT");
        let mut bytes = vec![0u8; ultima3::PARTY_LEN];
        bytes[0x00] = 0x3F; // transport: On Foot
        bytes[0x07] = 1; // party size: 1
        bytes[0x12..0x16].copy_from_slice(b"Bob\0"); // member 0 name
        std::fs::write(&path, &bytes).unwrap();
        let app = App::new(vec![GameRow {
            id: "u3".to_string(),
            title: "Ultima III".to_string(),
            kind: GameKind::Ultima3,
            inspectable: true,
            save_dir: Some(dir.path().to_path_buf()),
            files: vec![SaveFile {
                name: "PARTY.ULT".to_string(),
                path,
                found: true,
            }],
        }]);
        (dir, app)
    }

    /// An Ultima III game with both a roster and a party file present (triggers the chooser).
    fn app_with_ultima3_both() -> (tempfile::TempDir, App) {
        use fringe_retro_core::games::ultima3;
        let dir = tempfile::tempdir().unwrap();
        let mut roster = vec![0u8; ultima3::ROSTER_LEN];
        roster[0..4].copy_from_slice(b"Ari\0"); // slot 0 occupied
        std::fs::write(dir.path().join("ROSTER.ULT"), &roster).unwrap();
        let mut party = vec![0u8; ultima3::PARTY_LEN];
        party[0x00] = 0x3F;
        party[0x07] = 1;
        party[0x12..0x16].copy_from_slice(b"Bob\0");
        std::fs::write(dir.path().join("PARTY.ULT"), &party).unwrap();
        let files = ["ROSTER.ULT", "PARTY.ULT"]
            .iter()
            .map(|name| {
                let path = dir.path().join(name);
                let found = path.exists();
                SaveFile {
                    name: name.to_string(),
                    path,
                    found,
                }
            })
            .collect();
        let app = App::new(vec![GameRow {
            id: "u3".to_string(),
            title: "Ultima III".to_string(),
            kind: GameKind::Ultima3,
            inspectable: true,
            save_dir: Some(dir.path().to_path_buf()),
            files,
        }]);
        (dir, app)
    }

    fn select_field(app: &mut App, key: &str) {
        if let Some(Screen::Edit(ed)) = app.stack.last_mut() {
            let fi = ed.rows.iter().position(|r| r.key == key).unwrap();
            let di = ed
                .display
                .iter()
                .position(|d| matches!(d, DisplayRow::Field(i) if *i == fi))
                .unwrap();
            ed.list.select(Some(di));
        }
    }

    fn editor_value(app: &App, key: &str) -> String {
        match app.stack.last() {
            Some(Screen::Edit(ed)) => ed.rows.iter().find(|r| r.key == key).unwrap().value.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn single_char_game_opens_editor() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        assert!(matches!(app.stack.last(), Some(Screen::Edit(_))));
    }

    #[test]
    fn resources_key_opens_link_list_for_a_game() {
        // `ultima4` has bundled web resources.
        let games = vec![GameRow {
            id: "ultima4".to_string(),
            title: "Ultima IV".to_string(),
            kind: GameKind::Ultima4,
            inspectable: true,
            save_dir: None,
            files: Vec::new(),
        }];
        let mut app = App::new(games);
        app.handle_key(KeyCode::Char('r'));
        match app.stack.last() {
            Some(Screen::Resources(rl)) => {
                assert!(!rl.entries.is_empty());
                assert!(rl.entries.iter().all(|e| e.url.starts_with("https://")));
            }
            _ => panic!("expected the resources screen"),
        }
        app.handle_key(KeyCode::Esc); // back to the games list
        assert!(matches!(app.stack.last(), Some(Screen::Games(_))));
    }

    #[test]
    fn resources_key_shows_message_when_game_has_none() {
        let games = vec![GameRow {
            id: "nonesuch".to_string(),
            title: "Nonesuch".to_string(),
            kind: GameKind::Ultima1,
            inspectable: true,
            save_dir: None,
            files: Vec::new(),
        }];
        let mut app = App::new(games);
        app.handle_key(KeyCode::Char('r'));
        assert!(matches!(app.stack.last(), Some(Screen::Inspect(_))));
    }

    #[test]
    fn library_key_without_config_shows_message() {
        let mut app = app_with(1); // no [library] configured
        app.handle_key(KeyCode::Char('L'));
        assert!(matches!(app.stack.last(), Some(Screen::Inspect(_))));
    }

    #[test]
    fn item_add_picker_filters_and_selects() {
        let catalog = vec![
            (1u8, "Ax"),
            (13, "M1911A1 45 pistol"),
            (23, "AK 97 assault rifle"),
        ];
        let mut ia = ItemAdd::new(catalog);
        assert_eq!(ia.matches.len(), 3);
        assert_eq!(ia.selected(), Some((1, "Ax")));
        // Filtering narrows the list (case-insensitive substring) and re-tops the selection.
        ia.filter.push_str("PISTOL");
        ia.refilter();
        assert_eq!(ia.matches.len(), 1);
        assert_eq!(ia.selected(), Some((13, "M1911A1 45 pistol")));
        // A filter with no matches has no selection.
        ia.filter = "zzz".to_string();
        ia.refilter();
        assert!(ia.matches.is_empty());
        assert_eq!(ia.selected(), None);
    }

    #[test]
    fn library_add_then_delete_flow() {
        use fringe_retro_core::games::ultima1;
        let save_dir = tempfile::tempdir().unwrap();
        let lib_dir = tempfile::tempdir().unwrap();
        let path = save_dir.path().join("PLAYER1.U1");
        let mut bytes = vec![0u8; ultima1::SAVE_LEN];
        bytes[0..4].copy_from_slice(b"Enki");
        std::fs::write(&path, &bytes).unwrap();

        let mut app = App::new(vec![GameRow {
            id: "ultima1".to_string(),
            title: "Ultima I".to_string(),
            kind: GameKind::Ultima1,
            inspectable: true,
            save_dir: Some(save_dir.path().to_path_buf()),
            files: vec![SaveFile {
                name: "PLAYER1.U1".to_string(),
                path,
                found: true,
            }],
        }]);
        app.library = Some(Library::new(lib_dir.path()));

        // Open the (empty) library browser.
        app.handle_key(KeyCode::Char('L'));
        match app.stack.last() {
            Some(Screen::Library(ll)) => assert!(ll.entries.is_empty()),
            _ => panic!("expected the library screen"),
        }

        // Add a snapshot: 'a', type a name, Enter.
        app.handle_key(KeyCode::Char('a'));
        for c in "My Save".chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Enter);
        match app.stack.last() {
            Some(Screen::Library(ll)) => {
                assert_eq!(ll.entries.len(), 1);
                assert_eq!(ll.entries[0].name, "My Save");
                assert_eq!(ll.entries[0].slug, "my-save");
            }
            _ => panic!("expected the library screen after add"),
        }

        // Delete it: 'd' opens a confirm, 'y' carries it out.
        app.handle_key(KeyCode::Char('d'));
        assert!(matches!(app.stack.last(), Some(Screen::Confirm(_))));
        app.handle_key(KeyCode::Char('y'));
        match app.stack.last() {
            Some(Screen::Library(ll)) => assert!(ll.entries.is_empty()),
            _ => panic!("expected the library screen after delete"),
        }
    }

    #[test]
    fn u1_editor_groups_fields_into_sections() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        match app.stack.last() {
            Some(Screen::Edit(ed)) => {
                assert!(ed.display.len() > ed.rows.len()); // section headers were inserted
                assert!(matches!(ed.display.first(), Some(DisplayRow::Header(_))));
                assert!(ed.selected_field().is_some()); // selection starts on a field
            }
            _ => panic!("expected the editor"),
        }
    }

    #[test]
    fn u3_party_header_is_editable() {
        let (_dir, mut app) = app_with_ultima3_party();
        app.handle_key(KeyCode::Enter); // characters list: "Party settings" + members
        assert!(matches!(app.stack.last(), Some(Screen::Characters(_))));
        app.handle_key(KeyCode::Enter); // open "Party settings" (entity 0)
        match app.stack.last() {
            Some(Screen::Edit(ed)) => {
                assert!(ed.rows.iter().any(|r| r.key == "transport"));
                assert!(ed
                    .display
                    .iter()
                    .any(|d| matches!(d, DisplayRow::Header(_))));
            }
            _ => panic!("expected the party-header editor"),
        }
        select_field(&mut app, "transport"); // enum field -> picker
        app.handle_key(KeyCode::Enter);
        app.handle_key(KeyCode::Right); // On Foot -> Horse
        app.handle_key(KeyCode::Enter); // commit
        assert_eq!(editor_value(&app, "transport"), "Horse");
        assert!(app.session_dirty());
    }

    #[test]
    fn multi_file_game_shows_chooser_and_back_returns_to_it() {
        let (_dir, mut app) = app_with_ultima3_both();
        app.handle_key(KeyCode::Enter); // game has 2 files -> file chooser
        assert!(matches!(app.stack.last(), Some(Screen::SaveFiles(_))));

        app.handle_key(KeyCode::Down); // select PARTY.ULT
        app.handle_key(KeyCode::Enter); // open it -> characters (Party settings + members)
        assert!(matches!(app.stack.last(), Some(Screen::Characters(_))));

        app.handle_key(KeyCode::Esc); // back returns to the file chooser, not the games list
        assert!(matches!(app.stack.last(), Some(Screen::SaveFiles(_))));
        assert!(app.session.is_none()); // leaving the file cleared the session

        app.handle_key(KeyCode::Esc); // back again -> games list
        assert!(matches!(app.stack.last(), Some(Screen::Games(_))));
    }

    #[test]
    fn single_file_game_skips_the_chooser() {
        let (_dir, mut app) = app_with_ultima3_party(); // only PARTY.ULT present
        app.handle_key(KeyCode::Enter); // one file -> straight to characters, no chooser
        assert!(matches!(app.stack.last(), Some(Screen::Characters(_))));
    }

    #[test]
    fn edit_then_save_writes_once() {
        let (dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // open -> editor
        select_field(&mut app, "gold");
        app.handle_key(KeyCode::Enter); // begin edit (input seeded with "0")
        app.handle_key(KeyCode::Backspace); // clear the "0"
        for c in "500".chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Enter); // commit
        assert!(app.session_dirty());
        assert_eq!(editor_value(&app, "gold"), "500");

        app.handle_key(KeyCode::Char('s')); // save once
        assert!(!app.session_dirty());

        // Reload from disk: the edit is present in the written file.
        let path = dir.path().join("PLAYER1.U1");
        let session = crate::edit::Session::load(&path).unwrap().unwrap();
        let gold = session
            .rows(0)
            .into_iter()
            .find(|r| r.key == "gold")
            .unwrap();
        assert_eq!(gold.value, "500");
    }

    #[test]
    fn invalid_edit_keeps_input_and_shows_error() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        select_field(&mut app, "gold");
        app.handle_key(KeyCode::Enter);
        app.handle_key(KeyCode::Backspace);
        for c in "abc".chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Enter); // commit an invalid value
        assert!(!app.session_dirty());
        match app.stack.last() {
            Some(Screen::Edit(ed)) => {
                assert!(ed.input.is_some()); // input kept for correction
                assert!(ed.status.is_some()); // an error message is shown
            }
            _ => panic!("expected the editor"),
        }
    }

    #[test]
    fn numeric_field_uses_text_input() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        select_field(&mut app, "gold");
        app.handle_key(KeyCode::Enter); // begin edit
        match app.stack.last() {
            Some(Screen::Edit(ed)) => {
                assert!(ed.input.is_some());
                assert!(ed.picker.is_none());
            }
            _ => panic!("expected the editor"),
        }
    }

    #[test]
    fn enum_field_opens_picker_and_commits() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        select_field(&mut app, "race"); // enum: Human, Elf, Dwarf, Bobbit
        app.handle_key(KeyCode::Enter); // begin edit -> picker
        match app.stack.last() {
            Some(Screen::Edit(ed)) => {
                assert!(ed.picker.is_some());
                assert!(ed.input.is_none());
            }
            _ => panic!("expected the editor"),
        }
        app.handle_key(KeyCode::Right); // Human -> Elf
        app.handle_key(KeyCode::Enter); // commit
        assert_eq!(editor_value(&app, "race"), "Elf");
        assert!(app.session_dirty());
        match app.stack.last() {
            Some(Screen::Edit(ed)) => assert!(ed.picker.is_none()), // picker closed
            _ => panic!("expected the editor"),
        }
    }

    #[test]
    fn picker_wraps_and_esc_cancels() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        select_field(&mut app, "race");
        app.handle_key(KeyCode::Enter); // picker at "Human" (index 0)
        app.handle_key(KeyCode::Left); // wrap to last option "Bobbit"
        app.handle_key(KeyCode::Esc); // cancel without committing
        assert!(!app.session_dirty());
        assert_eq!(editor_value(&app, "race"), "Human"); // unchanged
    }

    #[test]
    fn quitting_with_unsaved_edits_prompts_then_discards() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        select_field(&mut app, "gold");
        app.handle_key(KeyCode::Enter);
        app.handle_key(KeyCode::Backspace);
        app.handle_key(KeyCode::Char('9'));
        app.handle_key(KeyCode::Enter); // commit -> dirty
        assert!(app.session_dirty());

        app.handle_key(KeyCode::Char('q')); // guarded: shows a prompt, does not quit
        assert!(matches!(app.stack.last(), Some(Screen::Confirm(_))));
        assert!(app.running);

        app.handle_key(KeyCode::Char('d')); // discard and quit
        assert!(!app.running);
    }

    /// Edit a field through the UI (begin edit, clear, type, commit).
    fn set_field_via_ui(app: &mut App, key: &str, value: &str) {
        select_field(app, key);
        app.handle_key(KeyCode::Enter); // begin edit
        for _ in 0..12 {
            app.handle_key(KeyCode::Backspace); // clear the seeded value
        }
        for c in value.chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Enter); // commit
    }

    #[test]
    fn backups_screen_handles_no_backups() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // editor
        app.handle_key(KeyCode::Char('b')); // no backups exist yet
        assert!(matches!(app.stack.last(), Some(Screen::Backups(_))));
        app.handle_key(KeyCode::Enter); // request restore with nothing selected -> no-op
        assert!(matches!(app.stack.last(), Some(Screen::Backups(_))));
        app.handle_key(KeyCode::Esc); // back to the editor
        assert!(matches!(app.stack.last(), Some(Screen::Edit(_))));
    }

    #[test]
    fn restore_backup_from_editor_reloads_values() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // editor (gold starts at 0)

        // Edit + save twice: the newest backup captures the pre-save value (gold 100).
        set_field_via_ui(&mut app, "gold", "100");
        app.handle_key(KeyCode::Char('s'));
        set_field_via_ui(&mut app, "gold", "200");
        app.handle_key(KeyCode::Char('s'));
        assert_eq!(editor_value(&app, "gold"), "200");

        app.handle_key(KeyCode::Char('b')); // open backups (newest = gold 100)
        assert!(matches!(app.stack.last(), Some(Screen::Backups(_))));
        app.handle_key(KeyCode::Enter); // request restore -> confirm
        assert!(matches!(app.stack.last(), Some(Screen::Confirm(_))));
        app.handle_key(KeyCode::Char('y')); // confirm restore

        // Back in the editor, reloaded from the restored file.
        assert!(matches!(app.stack.last(), Some(Screen::Edit(_))));
        assert_eq!(editor_value(&app, "gold"), "100");
        assert!(!app.session_dirty());
    }

    #[test]
    fn restore_confirm_previews_the_diff() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        set_field_via_ui(&mut app, "gold", "100");
        app.handle_key(KeyCode::Char('s')); // disk gold 100, backup gold 0
        set_field_via_ui(&mut app, "gold", "200");
        app.handle_key(KeyCode::Char('s')); // disk gold 200, newest backup gold 100

        app.handle_key(KeyCode::Char('b')); // backups (newest = gold 100)
        app.handle_key(KeyCode::Enter); // request restore -> confirm with a diff preview
        match app.stack.last() {
            Some(Screen::Confirm(c)) => {
                let msg = c.message.as_deref().unwrap_or_default();
                assert!(msg.contains("Restoring will change:"));
                assert!(msg.contains("200 -> 100")); // current 200 -> backup 100
            }
            _ => panic!("expected the confirm screen"),
        }
    }

    fn backups_len(app: &App) -> usize {
        match app.stack.last() {
            Some(Screen::Backups(bl)) => bl.entries.len(),
            _ => 0,
        }
    }

    #[test]
    fn snapshot_from_browser_creates_then_dedupes() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // editor (gold 0)
        set_field_via_ui(&mut app, "gold", "100");
        app.handle_key(KeyCode::Char('s')); // save: backup of gold 0, disk now gold 100

        app.handle_key(KeyCode::Char('b')); // browser: the save produced one backup
        let before = backups_len(&app);

        app.handle_key(KeyCode::Char('n')); // snapshot the on-disk save (gold 100)
        assert_eq!(backups_len(&app), before + 1);

        app.handle_key(KeyCode::Char('n')); // identical file -> no-op
        assert_eq!(backups_len(&app), before + 1);
    }

    fn app_with_template(fields: Vec<(&str, &str)>, game: &str) -> (tempfile::TempDir, App) {
        let (dir, mut app) = app_with_ultima1();
        let tpl = crate::templates::Template {
            game: game.to_string(),
            name: "Fighter".to_string(),
            description: Some("test template".to_string()),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        app.templates = TemplateSet::from_templates(vec![tpl]);
        (dir, app)
    }

    #[test]
    fn no_templates_for_game_shows_message() {
        let (_dir, mut app) = app_with_ultima1(); // no templates configured
        app.handle_key(KeyCode::Enter); // editor
        app.handle_key(KeyCode::Char('t'));
        assert!(matches!(app.stack.last(), Some(Screen::Inspect(_))));
    }

    #[test]
    fn apply_valid_template_updates_fields_and_dirties() {
        let (_dir, mut app) =
            app_with_template(vec![("gold", "250"), ("strength", "30")], "ultima1");
        app.handle_key(KeyCode::Enter); // editor
        app.handle_key(KeyCode::Char('t')); // template picker
        assert!(matches!(app.stack.last(), Some(Screen::Templates(_))));

        app.handle_key(KeyCode::Enter); // apply the (only, valid) template
        assert!(matches!(app.stack.last(), Some(Screen::Edit(_))));
        assert_eq!(editor_value(&app, "gold"), "250");
        assert_eq!(editor_value(&app, "strength"), "30");
        assert!(app.session_dirty()); // applied but not yet saved
    }

    #[test]
    fn invalid_template_is_flagged_and_not_applied() {
        // Gold's max is 9999, so this template fails validation.
        let (_dir, mut app) = app_with_template(vec![("gold", "999999")], "ultima1");
        app.handle_key(KeyCode::Enter); // editor
        app.handle_key(KeyCode::Char('t'));
        match app.stack.last() {
            Some(Screen::Templates(tl)) => assert!(tl.entries[0].error.is_some()),
            _ => panic!("expected the template picker"),
        }
        app.handle_key(KeyCode::Enter); // attempt to apply -> refused
        assert!(matches!(app.stack.last(), Some(Screen::Templates(_))));
        assert!(!app.session_dirty());
    }

    fn capture_selection(app: &App) -> Vec<bool> {
        match app.stack.last() {
            Some(Screen::Edit(ed)) => ed.capture.as_ref().unwrap().selected.clone(),
            _ => panic!("expected the editor in capture mode"),
        }
    }

    #[test]
    fn capture_prechecks_edited_fields() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // editor
        set_field_via_ui(&mut app, "gold", "100"); // marks `gold` edited
        app.handle_key(KeyCode::Char('T')); // begin capture

        let flags = capture_selection(&app);
        let (gold_i, name_i) = match app.stack.last() {
            Some(Screen::Edit(ed)) => (
                ed.rows.iter().position(|r| r.key == "gold").unwrap(),
                ed.rows.iter().position(|r| r.key == "name").unwrap(),
            ),
            _ => panic!("expected the editor"),
        };
        assert!(flags[gold_i]); // edited field pre-checked
        assert!(!flags[name_i]); // untouched field not checked
    }

    #[test]
    fn capture_toggle_all_and_enter_starts_naming() {
        let (_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter);
        app.handle_key(KeyCode::Char('T')); // capture, nothing pre-checked
        assert!(capture_selection(&app).iter().all(|s| !*s));

        app.handle_key(KeyCode::Char('a')); // select all
        assert!(capture_selection(&app).iter().all(|s| *s));

        app.handle_key(KeyCode::Enter); // proceed to naming
        match app.stack.last() {
            Some(Screen::Edit(ed)) => assert!(ed.capture.as_ref().unwrap().naming.is_some()),
            _ => panic!("expected the editor"),
        }
    }

    #[test]
    fn capture_writes_template_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let templates_path = dir.path().join("templates.toml");
        std::env::set_var("FRINGE_RETRO_TEMPLATES", &templates_path);

        let (_save_dir, mut app) = app_with_ultima1();
        app.handle_key(KeyCode::Enter); // editor
        set_field_via_ui(&mut app, "gold", "777"); // one edited field, pre-checked
        app.handle_key(KeyCode::Char('T')); // capture (gold pre-checked)
        app.handle_key(KeyCode::Enter); // -> naming
        for c in "Rich".chars() {
            app.handle_key(KeyCode::Char(c));
        }
        app.handle_key(KeyCode::Enter); // save template

        // Capture mode ended and the new template is now loaded and applicable.
        match app.stack.last() {
            Some(Screen::Edit(ed)) => assert!(ed.capture.is_none()),
            _ => panic!("expected the editor"),
        }
        let written = std::fs::read_to_string(&templates_path).unwrap();
        assert!(written.contains("name = \"Rich\""));
        assert!(written.contains("gold = 777"));
        assert!(!app.templates.for_game("ultima1").is_empty());

        std::env::remove_var("FRINGE_RETRO_TEMPLATES");
    }

    #[test]
    fn q_quits_from_either_screen() {
        let mut app = app_with(1);
        app.handle_key(KeyCode::Enter);
        app.handle_key(KeyCode::Char('q'));
        assert!(!app.running);
    }

    #[test]
    fn inspector_scroll_clamps() {
        let mut insp = Inspector {
            title: "t".to_string(),
            content: vec!["a".to_string(), "b".to_string()],
            scroll: 0,
            viewport: 1,
        };
        insp.scroll_up();
        assert_eq!(insp.scroll, 0); // can't scroll above the top
        insp.scroll_down();
        assert_eq!(insp.scroll, 1);
        insp.scroll_down();
        assert_eq!(insp.scroll, 1); // clamped at the last visible line
    }

    #[test]
    fn inspector_paging_and_jumps() {
        // 100 lines in a 10-line viewport: max scroll is 90.
        let content: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let mut insp = Inspector {
            title: "t".to_string(),
            content,
            scroll: 0,
            viewport: 10,
        };
        assert_eq!(insp.max_scroll(), 90);
        insp.page_down();
        assert_eq!(insp.scroll, 9); // one page (viewport - 1 overlap)
        insp.end();
        assert_eq!(insp.scroll, 90); // last full page
        insp.page_down();
        assert_eq!(insp.scroll, 90); // can't page past the end
        insp.page_up();
        assert_eq!(insp.scroll, 81);
        insp.home();
        assert_eq!(insp.scroll, 0);
    }

    #[test]
    fn backup_preview_shows_diff_then_full_contents() {
        use fringe_retro_core::games::ultima1;
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("PLAYER1.U1.bak");
        let save = dir.path().join("PLAYER1.U1");

        let mut old = vec![0u8; ultima1::SAVE_LEN];
        old[0..4].copy_from_slice(b"Enki");
        old[0x24..0x26].copy_from_slice(&100u16.to_le_bytes()); // gold 100 (backup)
        std::fs::write(&backup, &old).unwrap();
        let mut new = old.clone();
        new[0x24..0x26].copy_from_slice(&500u16.to_le_bytes()); // gold 500 (current)
        std::fs::write(&save, &new).unwrap();

        let lines = backup_preview(&backup, &save);
        let text = lines.join("\n");
        // Diff section first: what changed from the backup to the current save.
        assert!(text.contains("Changes since this backup:"));
        assert!(lines.iter().any(|l| l.contains("100 -> 500")));
        // Then the backup's full contents (absolute values, incl. the name).
        assert!(text.contains("Backup contents"));
        assert!(text.contains("Enki"));
    }

    #[test]
    fn snapshot_preview_shows_per_file_diff_then_contents() {
        use fringe_retro_core::games::ultima1;
        let snap_dir = tempfile::tempdir().unwrap();
        let save_dir = tempfile::tempdir().unwrap();

        let mut snap = vec![0u8; ultima1::SAVE_LEN];
        snap[0..4].copy_from_slice(b"Enki");
        snap[0x24..0x26].copy_from_slice(&100u16.to_le_bytes()); // gold 100 (snapshot)
        std::fs::write(snap_dir.path().join("PLAYER1.U1"), &snap).unwrap();
        let mut current = snap.clone();
        current[0x24..0x26].copy_from_slice(&500u16.to_le_bytes()); // gold 500 (current save)
        std::fs::write(save_dir.path().join("PLAYER1.U1"), &current).unwrap();

        let item = SnapshotItem {
            name: "Save".to_string(),
            slug: "save".to_string(),
            notes: None,
            updated: None,
            label: "Save".to_string(),
            dir: snap_dir.path().to_path_buf(),
            files: vec!["PLAYER1.U1".to_string()],
        };

        let lines = snapshot_preview(Some(&item), Some(save_dir.path()));
        let text = lines.join("\n");
        // Per-file diff (snapshot 100 -> current 500) then the full snapshot contents.
        assert!(text.contains("Changes since this snapshot:"));
        assert!(lines.iter().any(|l| l.contains("100 -> 500")));
        assert!(text.contains("── Snapshot contents ──"));
        assert!(text.contains("Enki"));
    }
}

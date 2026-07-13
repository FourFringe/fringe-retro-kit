//! Interactive terminal UI (Phase 4), built with Ratatui + Crossterm using a small
//! Elm-style state/update/view loop. Launched when `fringe-retro` is run with no command.
//!
//! Iteration 1 is a read-only **browser**: a list of the games in your library manifest,
//! and a scrollable inspector for the selected game's save. Editing and the Save Library
//! come later (see `ROADMAP.md`).

use std::path::{Path, PathBuf};

use anyhow::Result;
use fringe_retro_core::backup;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::edit::{Entity, FieldRow, Session};
use crate::templates::TemplateSet;

/// One game shown in the browser.
struct GameRow {
    id: String,
    title: String,
    inspectable: bool,
    save_path: Option<PathBuf>,
    found: bool,
}

/// Which screen the browser is showing. The browser keeps a stack of these; the top is
/// the current view and `Esc` pops back to the previous one.
enum Screen {
    /// The list of games (selection is an index into `App::games`).
    Games(ListState),
    /// A list of a multi-character save's characters (roster slots / party members).
    Characters(CharList),
    /// A field editor for one character.
    Edit(Editor),
    /// A browser of the current save's timestamped backups, with a preview of each.
    Backups(BackupList),
    /// A picker of character templates to apply to the current character.
    Templates(TemplateList),
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
    list: ListState,
    /// `Some` while the selected field's value is being typed.
    input: Option<String>,
    /// A one-line message (last edit, validation error, or save result).
    status: Option<String>,
}

impl Editor {
    fn selected_row(&self) -> Option<&FieldRow> {
        self.list.selected().and_then(|i| self.rows.get(i))
    }

    /// Begin editing the selected field: seed the input with its current value, and for
    /// enum/letter fields show the numbered choices on the edit line.
    fn begin_edit(&mut self) {
        if let Some(row) = self.selected_row() {
            let value = row.value.clone();
            let hint = row.choice_hint();
            self.input = Some(value);
            self.status = hint;
        }
    }
}

/// What to do once a modal prompt is resolved.
#[derive(Clone)]
enum Pending {
    QuitApp,
    LeaveGame,
    /// Restore the given backup over the current save, then refresh the editor entity.
    Restore {
        backup: PathBuf,
        entity: usize,
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
    OpenEntry(Option<usize>),
    Commit(String),
    Save,
    OpenBackups,
    RequestRestore,
    Snapshot,
    OpenTemplates,
    ApplyTemplate,
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
            session: None,
            stack: vec![Screen::Games(list)],
            running: true,
        }
    }

    fn session_dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.is_dirty())
    }

    /// Handle a "go back" request, guarding unsaved edits when leaving a game.
    fn back(&mut self) {
        match self.stack.len() {
            0 | 1 => self.request_quit(),
            2 => {
                // Popping returns to the game list, i.e. we are leaving the current game.
                if self.session_dirty() {
                    self.stack.push(Screen::Confirm(Confirm {
                        pending: Pending::LeaveGame,
                        message: None,
                    }));
                } else {
                    self.stack.pop();
                    self.session = None;
                }
            }
            _ => {
                self.stack.pop();
            }
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
            Some(Pending::LeaveGame) => {
                while self.stack.len() > 1 {
                    self.stack.pop();
                }
                self.session = None;
            }
            Some(Pending::Restore { backup, entity }) => self.do_restore(backup, entity),
            None => {}
        }
    }

    /// Open the backup browser for the current session's save, previewing the newest backup.
    fn open_backups(&mut self) {
        let (path, entity, editor_title) = match (self.session.as_ref(), self.stack.last()) {
            (Some(s), Some(Screen::Edit(ed))) => {
                (s.path().to_path_buf(), ed.entity, ed.title.clone())
            }
            _ => return,
        };
        let entries = build_backup_entries(&path);
        let mut list = ListState::default();
        if !entries.is_empty() {
            list.select(Some(0));
        }
        let content = match entries.first() {
            Some(e) => backup_preview(&e.path),
            None => vec!["(no backups yet)".to_string()],
        };
        let preview = Inspector::new("Preview".to_string(), content);
        self.stack.push(Screen::Backups(BackupList {
            title: format!("Backups — {editor_title}"),
            entity,
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
        let message = if self.session_dirty() {
            Some("Restore this backup? Unsaved edits will be lost.".to_string())
        } else {
            Some("Restore this backup over the current save?".to_string())
        };
        self.stack.push(Screen::Confirm(Confirm {
            pending: Pending::Restore { backup, entity },
            message,
        }));
    }

    /// Restore a backup over the current save, reload the session, and refresh the editor.
    fn do_restore(&mut self, backup: PathBuf, entity: usize) {
        let Some(path) = self.session.as_ref().map(|s| s.path().to_path_buf()) else {
            return;
        };
        let outcome = backup::restore(&backup, &path);
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
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            if let Some(rows) = rows {
                ed.rows = rows;
            }
            ed.input = None;
            ed.status = Some(status);
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
            ed.input = None;
            ed.status = Some(status);
        }
    }

    /// Open the selected game: load an editing session and show its character(s).
    fn open_game(&mut self, index: usize) {
        let Some(row) = self.games.get(index) else {
            return;
        };
        let title = format!("{} ({})", row.title, row.id);
        let inspectable = row.inspectable;
        let found = row.found;
        let save_path = row.save_path.clone();
        let game_title = row.title.clone();

        if !inspectable {
            let msg = vec![format!("Editing {game_title} is not supported yet.")];
            self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            return;
        }
        let Some(path) = save_path else {
            let msg = vec!["No save directory configured for this game.".to_string()];
            self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            return;
        };
        if !found {
            let msg = vec![
                "Save file not found:".to_string(),
                path.display().to_string(),
            ];
            self.stack.push(Screen::Inspect(Inspector::new(title, msg)));
            return;
        }
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
        let mut list = ListState::default();
        if !rows.is_empty() {
            list.select(Some(0));
        }
        self.stack.push(Screen::Edit(Editor {
            entity,
            title,
            rows,
            list,
            input: None,
            status: None,
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
                    ed.status = Some(format!("Set {key} = {value}"));
                }
                Err(e) => ed.status = Some(e.to_string()), // keep input so it can be fixed
            }
        }
    }

    /// Save the current session (one backup + one write) from the editor.
    fn save_from_editor(&mut self) {
        let result = self.session.as_mut().map(|s| s.save());
        if let Some(Screen::Edit(ed)) = self.stack.last_mut() {
            ed.status = Some(match result {
                Some(Ok(backup)) => format!("Saved. Backup: {}", backup.display()),
                Some(Err(e)) => format!("Save failed: {e}"),
                None => "Nothing to save.".to_string(),
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
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    action = Action::OpenGame(list.selected());
                }
                _ => {}
            },
            Screen::Characters(cl) => {
                let len = cl.entities.len();
                match code {
                    KeyCode::Char('q') => action = Action::Quit,
                    KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                        action = Action::Back;
                    }
                    KeyCode::Down | KeyCode::Char('j') => select_wrap(&mut cl.list, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => select_wrap(&mut cl.list, len, -1),
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        action = Action::OpenEntry(cl.list.selected());
                    }
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
                } else {
                    let len = ed.rows.len();
                    match code {
                        KeyCode::Char('q') => action = Action::Quit,
                        KeyCode::Esc | KeyCode::Left => action = Action::Back,
                        KeyCode::Down | KeyCode::Char('j') => select_wrap(&mut ed.list, len, 1),
                        KeyCode::Up | KeyCode::Char('k') => select_wrap(&mut ed.list, len, -1),
                        KeyCode::Enter | KeyCode::Char('e') => ed.begin_edit(),
                        KeyCode::Char('s') => action = Action::Save,
                        KeyCode::Char('b') => action = Action::OpenBackups,
                        KeyCode::Char('t') => action = Action::OpenTemplates,
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
            Screen::Confirm(c) => match &c.pending {
                Pending::Restore { .. } => match code {
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
            Action::OpenEntry(Some(i)) => self.open_entry(i),
            Action::Commit(v) => self.commit_edit(v),
            Action::Save => self.save_from_editor(),
            Action::OpenBackups => self.open_backups(),
            Action::RequestRestore => self.request_restore(),
            Action::Snapshot => self.snapshot_current(),
            Action::OpenTemplates => self.open_templates(),
            Action::ApplyTemplate => self.apply_template_selected(),
            Action::ConfirmSave => self.confirm_save(),
            Action::ConfirmDiscard => self.confirm_discard(),
            Action::ConfirmAccept => self.complete_pending(),
            Action::ConfirmCancel => self.confirm_cancel(),
            Action::OpenGame(None) | Action::OpenEntry(None) => {}
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
            Screen::Characters(cl) => draw_characters(frame, chunks[0], cl),
            Screen::Edit(ed) => draw_editor(frame, chunks[0], ed, dirty),
            Screen::Backups(bl) => draw_backups(frame, chunks[0], bl),
            Screen::Templates(tl) => draw_templates(frame, chunks[0], tl),
            Screen::Inspect(insp) => {
                insp.viewport = chunks[0].height.saturating_sub(2);
                draw_inspector(frame, chunks[0], insp);
            }
            Screen::Confirm(c) => draw_confirm(frame, chunks[0], c),
        }

        frame.render_widget(
            Paragraph::new(bottom).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }
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
                } else if g.found {
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

fn draw_characters(frame: &mut Frame, area: Rect, cl: &mut CharList) {
    let items: Vec<ListItem> = cl
        .entities
        .iter()
        .map(|e| ListItem::new(Line::from(e.label.clone())))
        .collect();
    let widget = selectable_list(items, format!(" {} ", cl.title));
    frame.render_stateful_widget(widget, area, &mut cl.list);
}

fn draw_editor(frame: &mut Frame, area: Rect, ed: &mut Editor, dirty: bool) {
    let items: Vec<ListItem> = ed
        .rows
        .iter()
        .map(|r| ListItem::new(Line::from(format!("{:<16} {}", r.label, r.value))))
        .collect();
    let marker = if dirty { "● " } else { "" };
    let widget = selectable_list(items, format!(" {marker}{} ", ed.title));
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
fn backup_preview(path: &Path) -> Vec<String> {
    match std::fs::read(path) {
        Ok(bytes) => crate::inspect::inspect_lines(&bytes)
            .unwrap_or_else(|e| vec![format!("(cannot preview: {e})")]),
        Err(e) => vec![format!("(cannot read: {e})")],
    }
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
        Some(e) => backup_preview(&e.path),
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
        Screen::Games(_) => " ↑/↓ select · Enter open · q quit ".to_string(),
        Screen::Characters(_) => " ↑/↓ select · Enter open · Esc back · q quit ".to_string(),
        Screen::Edit(ed) => {
            if let Some(input) = &ed.input {
                let label = ed.selected_row().map(|r| r.label).unwrap_or("value");
                match &ed.status {
                    Some(s) => format!("  {label}: {input}_   {s}"),
                    None => format!("  {label}: {input}_   (Enter commit · Esc cancel)"),
                }
            } else if let Some(status) = &ed.status {
                format!("  {status}")
            } else {
                " ↑/↓ field · Enter/e edit · s save · b backups · t templates · Esc back · q quit "
                    .to_string()
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
    let games = config
        .games()?
        .into_iter()
        .map(|g| {
            let save_path = g.save_dir.map(|dir| dir.join(g.kind.default_save_file()));
            let found = save_path.as_ref().is_some_and(|p| p.exists());
            GameRow {
                id: g.id,
                title: g.kind.title().to_string(),
                inspectable: g.kind.is_inspectable(),
                save_path,
                found,
            }
        })
        .collect();

    let mut app = App::new(games);
    match TemplateSet::load() {
        Ok(set) => app.templates = set,
        Err(e) => app.template_error = Some(e.to_string()),
    }
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
                inspectable: true,
                save_path: None,
                found: false,
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
            inspectable: true,
            save_path: Some(path),
            found: true,
        }]);
        (dir, app)
    }

    fn select_field(app: &mut App, key: &str) {
        if let Some(Screen::Edit(ed)) = app.stack.last_mut() {
            let idx = ed.rows.iter().position(|r| r.key == key).unwrap();
            ed.list.select(Some(idx));
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
}

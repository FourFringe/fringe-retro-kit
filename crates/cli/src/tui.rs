//! Interactive terminal UI (Phase 4), built with Ratatui + Crossterm using a small
//! Elm-style state/update/view loop. Launched when `fringe-retro` is run with no command.
//!
//! Iteration 1 is a read-only **browser**: a list of the games in your library manifest,
//! and a scrollable inspector for the selected game's save. Editing and the Save Library
//! come later (see `ROADMAP.md`).

use std::path::PathBuf;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::edit::{Entity, FieldRow, Session};

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

/// What to do once an unsaved-changes prompt is resolved.
#[derive(Clone, Copy)]
enum Pending {
    QuitApp,
    LeaveGame,
}

/// An unsaved-changes prompt shown before quitting or leaving a game with edits.
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
    ConfirmSave,
    ConfirmDiscard,
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
            Some(Screen::Confirm(c)) => Some(c.pending),
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
            None => {}
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
                        _ => {}
                    }
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
            Screen::Confirm(_) => match code {
                KeyCode::Char('s') => action = Action::ConfirmSave,
                KeyCode::Char('d') => action = Action::ConfirmDiscard,
                KeyCode::Esc | KeyCode::Char('n') => action = Action::ConfirmCancel,
                _ => {}
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
            Action::ConfirmSave => self.confirm_save(),
            Action::ConfirmDiscard => self.confirm_discard(),
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
    let msg = c.message.as_deref().unwrap_or("You have unsaved changes.");
    let text = format!("{msg}\n\n[s] save and continue\n[d] discard changes\n[Esc] cancel");
    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Unsaved changes "),
    );
    frame.render_widget(widget, area);
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
                " ↑/↓ field · Enter/e edit · s save · Esc back · q quit ".to_string()
            }
        }
        Screen::Inspect(_) => " ↑/↓ scroll · PgUp/PgDn page · Esc back · q quit ".to_string(),
        Screen::Confirm(_) => " s save · d discard · Esc cancel ".to_string(),
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

    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, App::new(games));
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

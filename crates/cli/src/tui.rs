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
use crate::inspect::{browse, Browse, Entry};

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
    /// A list of a multi-character save's sub-views (roster slots / party members).
    Characters(CharList),
    /// A scrollable inspector for one save or one character.
    Inspect(Inspector),
}

/// A selectable list of a save's sub-views.
struct CharList {
    title: String,
    entries: Vec<Entry>,
    list: ListState,
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

/// The browser application state: a stack of screens (top is current) plus the game list.
struct App {
    games: Vec<GameRow>,
    stack: Vec<Screen>,
    running: bool,
}

/// What a keypress asked the app to do, applied after the screen borrow is released.
enum Action {
    None,
    Back,
    OpenGame(Option<usize>),
    OpenEntry(Option<usize>),
}

impl App {
    fn new(games: Vec<GameRow>) -> Self {
        let mut list = ListState::default();
        if !games.is_empty() {
            list.select(Some(0));
        }
        App {
            games,
            stack: vec![Screen::Games(list)],
            running: true,
        }
    }

    /// Pop to the previous screen, or quit if already at the top-level game list.
    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        } else {
            self.running = false;
        }
    }

    /// Open the selected game: drill into its characters, or show a single inspector.
    fn open_game(&mut self, index: usize) {
        let Some(row) = self.games.get(index) else {
            return;
        };
        let title = format!("{} ({})", row.title, row.id);
        match load_browse(row) {
            Browse::Single(lines) => {
                self.stack
                    .push(Screen::Inspect(Inspector::new(title, lines)));
            }
            Browse::Multi(entries) => {
                let mut list = ListState::default();
                if !entries.is_empty() {
                    list.select(Some(0));
                }
                self.stack.push(Screen::Characters(CharList {
                    title,
                    entries,
                    list,
                }));
            }
        }
    }

    /// Open one entry (character) from the current character list.
    fn open_entry(&mut self, index: usize) {
        let sub = match self.stack.last() {
            Some(Screen::Characters(cl)) => cl
                .entries
                .get(index)
                .map(|e| (e.label.clone(), e.lines.clone())),
            _ => None,
        };
        if let Some((title, lines)) = sub {
            self.stack
                .push(Screen::Inspect(Inspector::new(title, lines)));
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        if code == KeyCode::Char('q') {
            self.running = false;
            return;
        }
        let games_len = self.games.len();
        let mut action = Action::None;
        match self.stack.last_mut().expect("stack is never empty") {
            Screen::Games(list) => match code {
                KeyCode::Esc => action = Action::Back,
                KeyCode::Down | KeyCode::Char('j') => select_wrap(list, games_len, 1),
                KeyCode::Up | KeyCode::Char('k') => select_wrap(list, games_len, -1),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    action = Action::OpenGame(list.selected());
                }
                _ => {}
            },
            Screen::Characters(cl) => {
                let len = cl.entries.len();
                match code {
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
            Screen::Inspect(insp) => match code {
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
        }
        match action {
            Action::Back => self.back(),
            Action::OpenGame(Some(i)) => self.open_game(i),
            Action::OpenEntry(Some(i)) => self.open_entry(i),
            Action::None | Action::OpenGame(None) | Action::OpenEntry(None) => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());

        let hint = match self.stack.last().expect("stack is never empty") {
            Screen::Games(_) => " ↑/↓ select · Enter open · q quit ",
            Screen::Characters(_) => " ↑/↓ select · Enter open · Esc back · q quit ",
            Screen::Inspect(_) => {
                " ↑/↓ scroll · PgUp/PgDn page · Home/End jump · Esc back · q quit "
            }
        };

        let games = &self.games;
        match self.stack.last_mut().expect("stack is never empty") {
            Screen::Games(list) => draw_games(frame, chunks[0], games, list),
            Screen::Characters(cl) => draw_characters(frame, chunks[0], cl),
            Screen::Inspect(insp) => {
                insp.viewport = chunks[0].height.saturating_sub(2);
                draw_inspector(frame, chunks[0], insp);
            }
        }

        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
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
        .entries
        .iter()
        .map(|e| ListItem::new(Line::from(e.label.clone())))
        .collect();
    let widget = selectable_list(items, format!(" {} ", cl.title));
    frame.render_stateful_widget(widget, area, &mut cl.list);
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

/// Browse a game into views, or a friendly message when it can't be shown.
fn load_browse(row: &GameRow) -> Browse {
    if !row.inspectable {
        return Browse::Single(vec![format!(
            "Inspecting {} is not supported yet.",
            row.title
        )]);
    }
    let Some(path) = &row.save_path else {
        return Browse::Single(vec![
            "No save directory configured for this game.".to_string()
        ]);
    };
    if !row.found {
        return Browse::Single(vec![
            "Save file not found:".to_string(),
            path.display().to_string(),
        ]);
    }
    match std::fs::read(path) {
        Ok(bytes) => match browse(&bytes) {
            Ok(b) => b,
            Err(e) => Browse::Single(vec![format!("Could not parse save: {e}")]),
        },
        Err(e) => Browse::Single(vec![format!("Could not read save: {e}")]),
    }
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

    #[test]
    fn drills_into_characters_then_back() {
        let mut app = app_with(1);
        // Simulate a multi-character game by pushing a Characters screen.
        let entries = vec![
            Entry {
                label: "Slot 1".to_string(),
                lines: vec!["  Name  A".to_string()],
            },
            Entry {
                label: "Slot 2".to_string(),
                lines: vec!["  Name  B".to_string()],
            },
        ];
        let mut list = ListState::default();
        list.select(Some(0));
        app.stack.push(Screen::Characters(CharList {
            title: "Ultima III".to_string(),
            entries,
            list,
        }));
        app.handle_key(KeyCode::Down); // select Slot 2
        app.handle_key(KeyCode::Enter); // open it
        match app.stack.last() {
            Some(Screen::Inspect(insp)) => assert_eq!(insp.title, "Slot 2"),
            _ => panic!("expected an inspector"),
        }
        app.handle_key(KeyCode::Esc);
        assert!(matches!(app.stack.last(), Some(Screen::Characters(_))));
        app.handle_key(KeyCode::Esc);
        assert!(matches!(app.stack.last(), Some(Screen::Games(_))));
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

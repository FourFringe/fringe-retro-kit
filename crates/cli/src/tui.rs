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
use crate::inspect::inspect_lines;

/// One game shown in the browser.
struct GameRow {
    id: String,
    title: String,
    inspectable: bool,
    save_path: Option<PathBuf>,
    found: bool,
}

/// Which screen the browser is showing.
enum Screen {
    Games,
    Inspect(Inspector),
}

/// A scrollable inspection view of one save.
struct Inspector {
    title: String,
    content: Vec<String>,
    scroll: u16,
}

impl Inspector {
    fn scroll_down(&mut self) {
        let max = self.content.len().saturating_sub(1) as u16;
        self.scroll = (self.scroll + 1).min(max);
    }

    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
}

/// The browser application state.
struct App {
    games: Vec<GameRow>,
    list: ListState,
    screen: Screen,
    running: bool,
}

impl App {
    fn new(games: Vec<GameRow>) -> Self {
        let mut list = ListState::default();
        if !games.is_empty() {
            list.select(Some(0));
        }
        App {
            games,
            list,
            screen: Screen::Games,
            running: true,
        }
    }

    fn select_next(&mut self) {
        if self.games.is_empty() {
            return;
        }
        let next = (self.list.selected().unwrap_or(0) + 1) % self.games.len();
        self.list.select(Some(next));
    }

    fn select_prev(&mut self) {
        if self.games.is_empty() {
            return;
        }
        let i = self.list.selected().unwrap_or(0);
        let prev = if i == 0 { self.games.len() - 1 } else { i - 1 };
        self.list.select(Some(prev));
    }

    fn open_selected(&mut self) {
        let Some(i) = self.list.selected() else {
            return;
        };
        let row = &self.games[i];
        self.screen = Screen::Inspect(Inspector {
            title: format!("{} ({})", row.title, row.id),
            content: load_inspection(row),
            scroll: 0,
        });
    }

    fn handle_key(&mut self, code: KeyCode) {
        match &mut self.screen {
            Screen::Games => match code {
                KeyCode::Char('q') | KeyCode::Esc => self.running = false,
                KeyCode::Down | KeyCode::Char('j') => self.select_next(),
                KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.open_selected(),
                _ => {}
            },
            Screen::Inspect(insp) => match code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                    self.screen = Screen::Games;
                }
                KeyCode::Down | KeyCode::Char('j') => insp.scroll_down(),
                KeyCode::Up | KeyCode::Char('k') => insp.scroll_up(),
                _ => {}
            },
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());

        let hint = if matches!(self.screen, Screen::Games) {
            " ↑/↓ select · Enter open · q quit "
        } else {
            " ↑/↓ scroll · Esc back · q quit "
        };

        if matches!(self.screen, Screen::Games) {
            self.draw_games(frame, chunks[0]);
        } else if let Screen::Inspect(insp) = &self.screen {
            draw_inspector(frame, chunks[0], insp);
        }

        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }

    fn draw_games(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = if self.games.is_empty() {
            vec![ListItem::new(
                "No games configured. See config.example.toml.",
            )]
        } else {
            self.games
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
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Fringe Retro Kit — Games "),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, area, &mut self.list);
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

/// Build the inspection lines for a game, or a friendly message when it can't be shown.
fn load_inspection(row: &GameRow) -> Vec<String> {
    if !row.inspectable {
        return vec![format!("Inspecting {} is not supported yet.", row.title)];
    }
    let Some(path) = &row.save_path else {
        return vec!["No save directory configured for this game.".to_string()];
    };
    if !row.found {
        return vec![
            "Save file not found:".to_string(),
            path.display().to_string(),
        ];
    }
    match std::fs::read(path) {
        Ok(bytes) => match inspect_lines(&bytes) {
            Ok(lines) => lines,
            Err(e) => vec![format!("Could not parse save: {e}")],
        },
        Err(e) => vec![format!("Could not read save: {e}")],
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

    #[test]
    fn selection_wraps_both_ways() {
        let mut app = app_with(3);
        assert_eq!(app.list.selected(), Some(0));
        app.select_next();
        assert_eq!(app.list.selected(), Some(1));
        app.select_prev();
        app.select_prev();
        assert_eq!(app.list.selected(), Some(2)); // wrapped past 0
        app.select_next();
        assert_eq!(app.list.selected(), Some(0)); // wrapped past end
    }

    #[test]
    fn empty_list_has_no_selection() {
        let mut app = app_with(0);
        assert_eq!(app.list.selected(), None);
        app.select_next(); // must not panic
        assert_eq!(app.list.selected(), None);
    }

    #[test]
    fn enter_opens_inspector_and_esc_returns() {
        let mut app = app_with(1);
        app.handle_key(KeyCode::Enter);
        assert!(matches!(app.screen, Screen::Inspect(_)));
        app.handle_key(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Games));
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
        };
        insp.scroll_up();
        assert_eq!(insp.scroll, 0); // can't scroll above the top
        insp.scroll_down();
        assert_eq!(insp.scroll, 1);
        insp.scroll_down();
        assert_eq!(insp.scroll, 1); // clamped at the last line
    }
}

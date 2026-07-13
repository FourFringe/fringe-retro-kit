//! Per-game save-file support.
//!
//! Phase 1 hardcodes a single game, Ultima I. Additional games will be added as
//! sibling modules; a generic, data-driven engine is deferred until a few games have
//! been implemented by hand (see `ROADMAP.md`).

pub mod ultima1;
pub mod ultima2;
pub mod ultima3;
pub mod wasteland;

/// A game with built-in save support. Used to map a library-manifest identifier to a
/// parser and its default save file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameKind {
    Ultima1,
    Ultima2,
    Ultima3,
    Wasteland,
}

impl GameKind {
    /// Every supported game, in a stable order.
    pub const ALL: [GameKind; 4] = [
        GameKind::Ultima1,
        GameKind::Ultima2,
        GameKind::Ultima3,
        GameKind::Wasteland,
    ];

    /// The stable lowercase identifier (e.g. `ultima1`).
    pub fn id(self) -> &'static str {
        match self {
            GameKind::Ultima1 => "ultima1",
            GameKind::Ultima2 => "ultima2",
            GameKind::Ultima3 => "ultima3",
            GameKind::Wasteland => "wasteland",
        }
    }

    /// A human-readable title.
    pub fn title(self) -> &'static str {
        match self {
            GameKind::Ultima1 => "Ultima I",
            GameKind::Ultima2 => "Ultima II",
            GameKind::Ultima3 => "Ultima III",
            GameKind::Wasteland => "Wasteland",
        }
    }

    /// Parse a game identifier, case-insensitively.
    pub fn from_id(s: &str) -> Option<Self> {
        GameKind::ALL
            .into_iter()
            .find(|k| k.id().eq_ignore_ascii_case(s))
    }

    /// The default save file to open within a game's save directory.
    pub fn default_save_file(self) -> &'static str {
        match self {
            GameKind::Ultima1 => "PLAYER1.U1",
            GameKind::Ultima2 => "PLAYER",
            GameKind::Ultima3 => "ROSTER.ULT",
            GameKind::Wasteland => "GAME1",
        }
    }

    /// The known save files this game may keep in its save directory (the default first).
    /// Ultima III has two: the character roster and the active party (which alone holds the
    /// party header).
    pub fn save_files(self) -> &'static [&'static str] {
        match self {
            GameKind::Ultima1 => &["PLAYER1.U1"],
            GameKind::Ultima2 => &["PLAYER"],
            GameKind::Ultima3 => &["ROSTER.ULT", "PARTY.ULT"],
            GameKind::Wasteland => &["GAME1"],
        }
    }

    /// Whether the headless CLI can currently inspect/edit this game's saves.
    /// (Wasteland's encrypted records aren't wired into `inspect` yet.)
    pub fn is_inspectable(self) -> bool {
        !matches!(self, GameKind::Wasteland)
    }
}

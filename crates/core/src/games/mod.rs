//! Per-game save-file support.
//!
//! Phase 1 hardcodes a single game, Ultima I. Additional games will be added as
//! sibling modules; a generic, data-driven engine is deferred until a few games have
//! been implemented by hand (see `ROADMAP.md`).

pub mod ultima1;
pub mod ultima2;
pub mod ultima3;
pub mod ultima4;
pub mod ultima5;
pub mod wasteland;

/// A game with built-in save support. Used to map a library-manifest identifier to a
/// parser and its default save file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameKind {
    Ultima1,
    Ultima2,
    Ultima3,
    Ultima4,
    Ultima5,
    Wasteland,
}

impl GameKind {
    /// Every supported game, in a stable order.
    pub const ALL: [GameKind; 6] = [
        GameKind::Ultima1,
        GameKind::Ultima2,
        GameKind::Ultima3,
        GameKind::Ultima4,
        GameKind::Ultima5,
        GameKind::Wasteland,
    ];

    /// The stable lowercase identifier (e.g. `ultima1`).
    pub fn id(self) -> &'static str {
        match self {
            GameKind::Ultima1 => "ultima1",
            GameKind::Ultima2 => "ultima2",
            GameKind::Ultima3 => "ultima3",
            GameKind::Ultima4 => "ultima4",
            GameKind::Ultima5 => "ultima5",
            GameKind::Wasteland => "wasteland",
        }
    }

    /// A human-readable title.
    pub fn title(self) -> &'static str {
        match self {
            GameKind::Ultima1 => "Ultima I",
            GameKind::Ultima2 => "Ultima II",
            GameKind::Ultima3 => "Ultima III",
            GameKind::Ultima4 => "Ultima IV",
            GameKind::Ultima5 => "Ultima V",
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
            GameKind::Ultima4 => "PARTY.SAV",
            GameKind::Ultima5 => "SAVED.GAM",
            GameKind::Wasteland => "GAME1",
        }
    }

    /// The known save files this game may keep in its save directory (the default first).
    /// Ultima I has up to four character slots; Ultima III has two (the character roster and
    /// the active party, which alone holds the party header).
    pub fn save_files(self) -> &'static [&'static str] {
        match self {
            GameKind::Ultima1 => &["PLAYER1.U1", "PLAYER2.U1", "PLAYER3.U1", "PLAYER4.U1"],
            GameKind::Ultima2 => &["PLAYER"],
            GameKind::Ultima3 => &["ROSTER.ULT", "PARTY.ULT"],
            GameKind::Ultima4 => &["PARTY.SAV"],
            GameKind::Ultima5 => &["SAVED.GAM"],
            GameKind::Wasteland => &["GAME1"],
        }
    }

    /// Whether the headless CLI can currently inspect/edit this game's saves.
    /// (Wasteland's encrypted records aren't wired into `inspect` yet.)
    pub fn is_inspectable(self) -> bool {
        !matches!(self, GameKind::Wasteland)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_files_start_with_the_default() {
        for kind in GameKind::ALL {
            let files = kind.save_files();
            assert!(!files.is_empty());
            assert_eq!(files[0], kind.default_save_file());
        }
    }

    #[test]
    fn ultima1_lists_four_slots() {
        assert_eq!(GameKind::Ultima1.save_files().len(), 4);
    }
}

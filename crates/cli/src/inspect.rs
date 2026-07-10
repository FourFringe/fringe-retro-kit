//! Formats a save file into display lines. Shared by the `inspect` command and the TUI so
//! both show the same view. Auto-detects the game by file size, mirroring the CLI's other
//! commands.

use anyhow::Result;
use fringe_retro_core::games::ultima1::Ultima1Save;
use fringe_retro_core::games::ultima2::{self, Ultima2Save};
use fringe_retro_core::games::ultima3::{self, Ultima3Party, Ultima3Roster};

/// Produce the human-readable inspection lines for a save file's bytes.
pub fn inspect_lines(bytes: &[u8]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if bytes.len() == ultima3::PARTY_LEN {
        let party = Ultima3Party::from_bytes(bytes.to_vec())?;
        out.push("Party:".to_string());
        for (label, value) in party.header_inspect() {
            out.push(format!("  {label:<20} {value}"));
        }
        let order = party.party_order();
        let members = party.party_size().min(ultima3::PARTY_MEMBER_COUNT);
        for (member, slot) in order.iter().enumerate().take(members) {
            out.push(String::new());
            out.push(format!(
                "Member {} (roster slot {}): {}",
                member + 1,
                slot,
                party.summary(member)
            ));
            for (label, value) in party.inspect(member) {
                out.push(format!("  {label:<16} {value}"));
            }
        }
    } else if bytes.len() == ultima3::ROSTER_LEN {
        let roster = Ultima3Roster::from_bytes(bytes.to_vec())?;
        let occupied = roster.occupied_slots();
        if occupied.is_empty() {
            out.push("(empty roster)".to_string());
        }
        for slot in occupied {
            out.push(String::new());
            out.push(format!("Slot {}: {}", slot + 1, roster.summary(slot)));
            for (label, value) in roster.inspect(slot) {
                out.push(format!("  {label:<16} {value}"));
            }
        }
    } else if bytes.len() == ultima2::SAVE_LEN {
        let save = Ultima2Save::from_bytes(bytes.to_vec())?;
        out.push("Ultima II (partial — reverse-engineering in progress):".to_string());
        for (label, value, tentative) in save.inspect() {
            let mark = if tentative { "  (?)" } else { "" };
            out.push(format!("  {label:<12} {value}{mark}"));
        }
    } else {
        let save = Ultima1Save::from_bytes(bytes.to_vec())?;
        let mut current_section = "";
        for (section, label, value) in save.inspect() {
            if section != current_section {
                out.push(String::new());
                out.push(format!("{section}:"));
                current_section = section;
            }
            out.push(format!("  {label:<16} {value}"));
        }
    }
    Ok(out)
}

/// One named sub-view within a multi-character save (a roster slot or party member).
pub struct Entry {
    pub label: String,
    pub lines: Vec<String>,
}

/// How a save can be browsed: a single view, or a list of named sub-views to drill into.
pub enum Browse {
    Single(Vec<String>),
    Multi(Vec<Entry>),
}

/// Split a save into browsable pieces. Multi-character games (Ultima III roster/party)
/// return one [`Entry`] per character (plus a party overview); everything else is a single
/// view reusing [`inspect_lines`].
pub fn browse(bytes: &[u8]) -> Result<Browse> {
    if bytes.len() == ultima3::PARTY_LEN {
        let party = Ultima3Party::from_bytes(bytes.to_vec())?;
        let mut entries = Vec::new();
        let overview: Vec<String> = party
            .header_inspect()
            .into_iter()
            .map(|(label, value)| format!("  {label:<20} {value}"))
            .collect();
        entries.push(Entry {
            label: "Party overview".to_string(),
            lines: overview,
        });
        let order = party.party_order();
        let members = party.party_size().min(ultima3::PARTY_MEMBER_COUNT);
        for (member, slot) in order.iter().enumerate().take(members) {
            let mut lines = vec![format!("(roster slot {slot})"), String::new()];
            for (label, value) in party.inspect(member) {
                lines.push(format!("  {label:<16} {value}"));
            }
            entries.push(Entry {
                label: format!("{}. {}", member + 1, party.summary(member)),
                lines,
            });
        }
        Ok(Browse::Multi(entries))
    } else if bytes.len() == ultima3::ROSTER_LEN {
        let roster = Ultima3Roster::from_bytes(bytes.to_vec())?;
        let occupied = roster.occupied_slots();
        if occupied.is_empty() {
            return Ok(Browse::Single(vec!["(empty roster)".to_string()]));
        }
        let entries = occupied
            .into_iter()
            .map(|slot| {
                let lines = roster
                    .inspect(slot)
                    .into_iter()
                    .map(|(label, value)| format!("  {label:<16} {value}"))
                    .collect();
                Entry {
                    label: format!("Slot {}: {}", slot + 1, roster.summary(slot)),
                    lines,
                }
            })
            .collect();
        Ok(Browse::Multi(entries))
    } else {
        // Single-character games (Ultima I / II): one view.
        Ok(Browse::Single(inspect_lines(bytes)?))
    }
}

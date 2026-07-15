//! Formats a save file into display lines. Shared by the `inspect` command and the TUI so
//! both show the same view. Auto-detects the game by file size, mirroring the CLI's other
//! commands.

use anyhow::Result;
use fringe_retro_core::games::ultima1::Ultima1Save;
use fringe_retro_core::games::ultima2::{self, Ultima2Save};
use fringe_retro_core::games::ultima3::{self, Ultima3Party, Ultima3Roster};
use fringe_retro_core::games::ultima4::{self, Ultima4Save};
use fringe_retro_core::games::ultima5::{self, Ultima5Save};
use fringe_retro_core::games::wasteland::WastelandSave;

/// Produce the human-readable inspection lines for a save file's bytes.
pub fn inspect_lines(bytes: &[u8]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if bytes.starts_with(b"msq0") {
        let save = WastelandSave::from_bytes(bytes.to_vec())?;
        let occupied = save.occupied_characters();
        if occupied.is_empty() {
            out.push("(no characters in this save)".to_string());
        }
        for i in occupied {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(format!(
                "Character {}: {}",
                i + 1,
                save.character_summary(i)
            ));
            let mut current_section = "";
            for (section, label, value) in save.character_inspect(i) {
                if section != current_section {
                    out.push(format!("  {section}:"));
                    current_section = section;
                }
                out.push(format!("    {label:<16} {value}"));
            }
            let skills = save.skills(i);
            if !skills.is_empty() {
                out.push("  Skills:".to_string());
                for skill in skills {
                    out.push(format!("    {:<16} {}", skill.name, skill.level));
                }
            }
        }
    } else if bytes.len() == ultima3::PARTY_LEN {
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
            let mut current_section = "";
            for (section, label, value) in party.inspect(member) {
                if section != current_section {
                    out.push(format!("  {section}:"));
                    current_section = section;
                }
                out.push(format!("    {label:<16} {value}"));
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
            let mut current_section = "";
            for (section, label, value) in roster.inspect(slot) {
                if section != current_section {
                    out.push(format!("  {section}:"));
                    current_section = section;
                }
                out.push(format!("    {label:<16} {value}"));
            }
        }
    } else if bytes.len() == ultima4::SAVE_LEN {
        let save = Ultima4Save::from_bytes(bytes.to_vec())?;
        out.push("Party & Virtues:".to_string());
        let mut current_section = "";
        for (section, label, value) in save.party_inspect() {
            if section != current_section {
                out.push(format!("  {section}:"));
                current_section = section;
            }
            out.push(format!("    {label:<16} {value}"));
        }
        for i in save.occupied_players() {
            out.push(String::new());
            out.push(format!("Player {}: {}", i + 1, save.player_summary(i)));
            let mut current_section = "";
            for (section, label, value) in save.player_inspect(i) {
                if section != current_section {
                    out.push(format!("  {section}:"));
                    current_section = section;
                }
                out.push(format!("    {label:<16} {value}"));
            }
        }
    } else if bytes.len() == ultima5::SAVE_LEN {
        let save = Ultima5Save::from_bytes(bytes.to_vec())?;
        out.push("Party & Provisions:".to_string());
        let mut current_section = "";
        for (section, label, value) in save.party_inspect() {
            if section != current_section {
                out.push(format!("  {section}:"));
                current_section = section;
            }
            out.push(format!("    {label:<16} {value}"));
        }
        for i in save.occupied_characters() {
            out.push(String::new());
            out.push(format!(
                "Character {}: {}",
                i + 1,
                save.character_summary(i)
            ));
            let mut current_section = "";
            for (section, label, value) in save.character_inspect(i) {
                if section != current_section {
                    out.push(format!("  {section}:"));
                    current_section = section;
                }
                out.push(format!("    {label:<16} {value}"));
            }
        }
    } else if bytes.len() == ultima2::SAVE_LEN {
        let save = Ultima2Save::from_bytes(bytes.to_vec())?;
        out.push("Ultima II (partial — reverse-engineering in progress):".to_string());
        let mut current_section = "";
        for (section, label, value, tentative) in save.inspect() {
            if section != current_section {
                out.push(String::new());
                out.push(format!("{section}:"));
                current_section = section;
            }
            let mark = if tentative { "  (?)" } else { "" };
            out.push(format!("  {label:<16} {value}{mark}"));
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

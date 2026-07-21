//! Shared helpers for the kit tools: numeric parsing (decimal or `0x` hex, since offsets in
//! reverse-engineering are usually hex) and `xxd`-style hex-dump formatting.

/// Parse an unsigned integer written in decimal or `0x`/`0X`-prefixed hexadecimal.
fn parse_radix(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let parsed = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => s.parse::<u64>(),
    };
    parsed.map_err(|_| format!("'{s}' is not a valid number (use decimal or 0x-prefixed hex)"))
}

/// A clap value parser for a byte (`0..=255`), in decimal or hex.
pub fn u8_arg(s: &str) -> Result<u8, String> {
    let v = parse_radix(s)?;
    u8::try_from(v).map_err(|_| format!("'{s}' does not fit in a byte (0..=255)"))
}

/// A clap value parser for a `u32`, in decimal or hex.
pub fn u32_arg(s: &str) -> Result<u32, String> {
    let v = parse_radix(s)?;
    u32::try_from(v).map_err(|_| format!("'{s}' does not fit in 32 bits"))
}

/// A clap value parser for a `usize` (offset/length), in decimal or hex.
pub fn usize_arg(s: &str) -> Result<usize, String> {
    let v = parse_radix(s)?;
    usize::try_from(v).map_err(|_| format!("'{s}' is too large"))
}

/// Format `data` as a canonical hex dump: `offset  16 hex bytes  |ascii|`.
pub fn hexdump(data: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in data.chunks(16).enumerate() {
        let offset = row * 16;
        let hex = chunk
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        // 16 bytes * 2 hex digits + 15 separators = 47 columns.
        out.push_str(&format!("{offset:08x}  {hex:<47}  |{ascii}|\n"));
    }
    out
}

/// Lowercase hex string of `data` (no separators) — used for JSON output.
pub fn hex_string(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_and_hex() {
        assert_eq!(usize_arg("0x18290").unwrap(), 0x18290);
        assert_eq!(usize_arg("100").unwrap(), 100);
        assert_eq!(u8_arg("0x1f").unwrap(), 0x1f);
        assert!(u8_arg("256").is_err());
        assert!(usize_arg("nope").is_err());
    }

    #[test]
    fn hexdumps_with_ascii_gutter() {
        let dump = hexdump(b"AB\x01");
        assert!(dump.starts_with("00000000  41 42 01"));
        assert!(dump.trim_end().ends_with("|AB.|"));
    }
}

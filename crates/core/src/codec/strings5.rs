//! Wasteland's **5-bit packed strings**. Messages are stored as streams of 5-bit indices into a
//! 60-byte character table (two 30-entry halves). Two indices are escapes rather than characters:
//! `0x1F` selects the table's high half for the *next* character, and `0x1E` upper-cases it. A
//! decoded character of `0` ends the string. This mirrors `wlandsuite`'s `Strings`/`CharTable`;
//! only the raw bit/character machinery lives here — locating the table and offset list is the
//! caller's (game/map) job.

/// Index that selects the char table's high half for the next character.
const HIGH_HALF: u8 = 0x1F;
/// Index that upper-cases the next character.
const UPPER: u8 = 0x1E;
/// Number of entries in each half of the character table.
const HALF: usize = 0x1E;

/// A least-significant-bit-first bit reader, matching `wlandsuite`'s reverse-mode
/// `BitInputStream`. Reads run off the end of `data` as an exhausted stream rather than panicking.
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    bit: u8,
    exhausted: bool,
}

impl<'a> BitReader<'a> {
    /// Start reading at byte `pos` within `data`.
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        BitReader {
            data,
            pos,
            cur: 0,
            bit: 7,
            exhausted: false,
        }
    }

    /// Whether the reader has run past the end of `data`.
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }

    /// The next byte position to be read.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Read one bit, least-significant-bit first within each byte.
    fn read_bit(&mut self) -> u8 {
        if self.bit > 6 {
            match self.data.get(self.pos) {
                Some(&b) => self.cur = b,
                None => {
                    self.cur = 0;
                    self.exhausted = true;
                }
            }
            self.pos += 1;
            self.bit = 0;
        } else {
            self.bit += 1;
        }
        (self.cur >> self.bit) & 1
    }

    /// Read a 5-bit value, least-significant-bit first.
    pub fn read5(&mut self) -> u8 {
        (0..5).fold(0u8, |v, i| v | (self.read_bit() << i))
    }
}

/// Decode a single packed string from `reader` using the 60-byte `char_table`, stopping at a
/// terminating `0` character or when the reader is exhausted. `upper`/`high` escapes affect only
/// the character that follows them.
pub fn decode_string(char_table: &[u8], reader: &mut BitReader) -> String {
    let mut s = String::new();
    let (mut upper, mut high) = (false, false);
    loop {
        if reader.exhausted() {
            break;
        }
        match reader.read5() {
            HIGH_HALF => high = true,
            UPPER => upper = true,
            index => {
                let Some(&ch) = char_table.get(usize::from(index) + usize::from(high) * HALF)
                else {
                    break;
                };
                if ch == 0 {
                    break;
                }
                let c = ch as char;
                if upper {
                    s.extend(c.to_uppercase());
                } else {
                    s.push(c);
                }
                upper = false;
                high = false;
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_five_bits_lsb_first() {
        // 0x15 = 0b10101; read5 accumulates bits LSB-first, reproducing the low five bits.
        let data = [0x15];
        let mut bits = BitReader::new(&data, 0);
        assert_eq!(bits.read5(), 0x15 & 0x1F);
    }

    #[test]
    fn decodes_a_string_until_the_terminator() {
        // Char table: index 1 -> 'h', 2 -> 'i', 0 -> NUL terminator (index 0).
        let mut table = [0u8; 60];
        table[1] = b'h';
        table[2] = b'i';
        // Stream of 5-bit indices: 1 ('h'), 2 ('i'), 0 (end). LSB-first packing of 00001, 00010, 00000.
        // byte0 bits: idx1=00001, idx2=00010 -> bit layout LSB-first.
        let mut bits = 0u32;
        let mut n = 0;
        for idx in [1u8, 2, 0] {
            bits |= u32::from(idx) << n;
            n += 5;
        }
        let bytes = bits.to_le_bytes();
        let mut reader = BitReader::new(&bytes, 0);
        assert_eq!(decode_string(&table, &mut reader), "hi");
    }

    #[test]
    fn upper_escape_capitalises_next_character() {
        let mut table = [0u8; 60];
        table[1] = b'a';
        // indices: UPPER(0x1E), 1 ('a'->'A'), 0 (end).
        let mut bits = 0u32;
        let mut n = 0;
        for idx in [UPPER, 1, 0] {
            bits |= u32::from(idx) << n;
            n += 5;
        }
        let bytes = bits.to_le_bytes();
        let mut reader = BitReader::new(&bytes, 0);
        assert_eq!(decode_string(&table, &mut reader), "A");
    }
}

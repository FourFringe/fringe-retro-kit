//! **EXEPACK** decompression for DOS MZ executables. Many DOS games ship an EXEPACK-compressed
//! `.EXE` whose interesting tables (item/skill name strings, metadata) live in the *unpacked*
//! image — e.g. Wasteland's `WL.EXE`. This unpacks such a file into its decompressed **load
//! module** (the program image after the MZ header), so a string ripper or overlay can read it.
//!
//! The packed program is a backwards run-length stream: reading from the top of the packed region
//! downward, each command is a byte (`0xB0/0xB1` fill, `0xB2/0xB3` copy; the low bit marks the
//! final command), preceded by a little-endian 16-bit `count`, and — for a fill — a fill byte just
//! below that. The decompressor writes the output from its end downward. See the reference at
//! <https://www.bamsoftware.com/software/exepack/> for the format.

use crate::{Error, Result};

fn u16le(data: &[u8], offset: usize) -> Result<usize> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        .ok_or_else(|| Error::Format("EXEPACK: truncated (out of range word)".into()))
}

/// Unpack an EXEPACK-compressed MZ executable, returning its decompressed **load module** (the
/// program image, without the MZ header). Errors if the file isn't a valid EXEPACK image.
pub fn unpack(exe: &[u8]) -> Result<Vec<u8>> {
    if exe.get(0..2) != Some(b"MZ") {
        return Err(Error::Format("EXEPACK: not an MZ executable".into()));
    }
    // MZ header: bytes-on-last-page, page count, header paragraphs, and the entry CS (which points
    // at the EXEPACK header inside the load module).
    let e_cblp = u16le(exe, 2)?;
    let e_cp = u16le(exe, 4)?;
    let e_cparhdr = u16le(exe, 8)?;
    let e_cs = u16le(exe, 0x16)?;

    let header_len = e_cparhdr * 16;
    let exe_size = if e_cblp != 0 {
        e_cp.saturating_sub(1) * 512 + e_cblp
    } else {
        e_cp * 512
    };
    let end = exe_size.min(exe.len());
    let load = exe
        .get(header_len..end)
        .ok_or_else(|| Error::Format("EXEPACK: header extends past end of file".into()))?;

    // The EXEPACK variable header sits at CS:0 within the load module; the packed program is
    // everything before it.
    let header_off = e_cs * 16;
    if header_off + 14 > load.len() {
        return Err(Error::Format("EXEPACK: header offset out of range".into()));
    }
    let dest_len = u16le(load, header_off + 12)?; // unpacked length, in 16-byte paragraphs
    let packed = &load[..header_off];

    let mut dst = vec![0u8; dest_len * 16];
    let mut src = packed.len();
    let mut out = dst.len();

    // The packed region is padded up to a paragraph with 0xFF; skip that trailer first.
    while src > 0 && packed[src - 1] == 0xFF {
        src -= 1;
    }

    let short = || Error::Format("EXEPACK: truncated packed stream".into());
    let overflow = || Error::Format("EXEPACK: output overflow (corrupt stream)".into());
    loop {
        if src == 0 {
            break;
        }
        let cmd = packed[src - 1];
        src -= 1;
        if src < 2 {
            return Err(short());
        }
        let count = packed[src - 2] as usize | ((packed[src - 1] as usize) << 8);
        src -= 2;

        match cmd & 0xFE {
            0xB0 => {
                // Fill `count` bytes with the single byte just below the count.
                if src < 1 {
                    return Err(short());
                }
                let fill = packed[src - 1];
                src -= 1;
                if out < count {
                    return Err(overflow());
                }
                for _ in 0..count {
                    out -= 1;
                    dst[out] = fill;
                }
            }
            0xB2 => {
                // Copy `count` literal bytes (backwards, so they land in order).
                if src < count || out < count {
                    return Err(overflow());
                }
                for _ in 0..count {
                    out -= 1;
                    src -= 1;
                    dst[out] = packed[src];
                }
            }
            other => {
                return Err(Error::Format(format!(
                    "EXEPACK: unknown command {other:#04x}"
                )));
            }
        }

        if cmd & 1 == 1 {
            break; // final command
        }
    }

    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble a minimal EXEPACK MZ image around a paragraph-aligned packed region.
    fn packed_exe(packed: &[u8], dest_len_paras: u16) -> Vec<u8> {
        assert!(
            packed.len().is_multiple_of(16),
            "packed region must be paragraph-aligned"
        );
        let e_cparhdr = 2u16; // a 32-byte MZ header
        let header_len = e_cparhdr as usize * 16;
        let e_cs = (packed.len() / 16) as u16; // EXEPACK header starts right after the packed data

        // The 18-byte EXEPACK header: real IP/CS, mem start, exepack size, real SP/SS, dest_len,
        // skip_len, then the "RB" signature.
        let mut eph = Vec::new();
        for word in [0u16, 0, 0, 18, 0, 0, dest_len_paras, 1] {
            eph.extend_from_slice(&word.to_le_bytes());
        }
        eph.extend_from_slice(b"RB");

        let mut load = packed.to_vec();
        load.extend_from_slice(&eph);
        let exe_size = header_len + load.len();

        let mut exe = vec![0u8; header_len];
        exe[0..2].copy_from_slice(b"MZ");
        exe[2..4].copy_from_slice(&((exe_size % 512) as u16).to_le_bytes());
        exe[4..6].copy_from_slice(&(exe_size.div_ceil(512) as u16).to_le_bytes());
        exe[8..10].copy_from_slice(&e_cparhdr.to_le_bytes());
        exe[0x16..0x18].copy_from_slice(&e_cs.to_le_bytes());
        exe.extend_from_slice(&load);
        exe
    }

    #[test]
    fn unpacks_a_fill_command() {
        // low->high: [fill=0xAB][count_lo=16][count_hi=0][cmd=0xB1 fill+final], padded with 0xFF.
        let mut packed = vec![0xAB, 16, 0, 0xB1];
        packed.resize(16, 0xFF);
        let out = unpack(&packed_exe(&packed, 1)).unwrap();
        assert_eq!(out, vec![0xAB; 16]);
    }

    #[test]
    fn unpacks_a_copy_command() {
        // low->high: [b0..b15][count_lo=16][count_hi=0][cmd=0xB3 copy+final], padded with 0xFF.
        let mut packed: Vec<u8> = (0..16).collect();
        packed.extend_from_slice(&[16, 0, 0xB3]);
        while !packed.len().is_multiple_of(16) {
            packed.push(0xFF);
        }
        let out = unpack(&packed_exe(&packed, 1)).unwrap();
        assert_eq!(out, (0u8..16).collect::<Vec<_>>());
    }

    #[test]
    fn rejects_non_mz() {
        assert!(unpack(b"not an exe").is_err());
    }
}

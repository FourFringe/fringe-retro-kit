//! The **archive extractor**: carve a container file into its member blocks by a magic signature.
//! Many retro data files (e.g. Wasteland's back-to-back `msq` blocks) are a run of self-delimiting
//! records with no index, so splitting at each signature is the way in. Lists blocks by default;
//! `--out` writes each one to a numbered file.

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use fringe_retro_core::games::wasteland;
use fringe_retro_core::scan::{self, Segment};
use serde::Serialize;

use crate::util;

#[derive(Args)]
pub struct CarveArgs {
    /// Container file to carve.
    file: PathBuf,
    /// Signature that starts each block: literal ASCII (e.g. `msq`) or `0x` hex (e.g. `0x6d7371`).
    /// Defaults to `msq` when `--decrypt` is set.
    #[arg(long)]
    magic: Option<String>,
    /// Skip segments shorter than this many bytes (filters spurious matches).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    min_size: usize,
    /// Treat each block as a Wasteland MSQ block: strip the `msqN` + seed header and undo the
    /// rolling-XOR cipher before listing/extracting. Implies `--magic msq`.
    #[arg(long)]
    decrypt: bool,
    /// Keep only the Wasteland *savegame* block (the party/character record). Implies `--decrypt`.
    #[arg(long)]
    savegame_only: bool,
    /// Directory to write each block into (created if needed). Without it, blocks are only listed.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Emit JSON instead of a listing.
    #[arg(long)]
    json: bool,
}

pub fn run(args: CarveArgs) -> Result<()> {
    let decrypt = args.decrypt || args.savegame_only;
    let magic = match &args.magic {
        Some(spec) => util::bytes_arg(spec).map_err(|e| anyhow!(e))?,
        None if decrypt => b"msq".to_vec(),
        None => bail!("--magic is required (or use --decrypt for Wasteland MSQ blocks)"),
    };
    if magic.is_empty() {
        bail!("--magic must not be empty");
    }
    if decrypt && magic != b"msq" {
        bail!("--decrypt only applies to Wasteland MSQ blocks; use --magic msq (or omit --magic)");
    }
    let bytes = fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let mut segments: Vec<Segment> = scan::carve(&bytes, &magic)
        .into_iter()
        .filter(|s| s.len >= args.min_size)
        .collect();
    if args.savegame_only {
        segments.retain(|s| wasteland::is_savegame_block(&bytes[s.offset..s.offset + s.len]));
    }

    // The bytes each block contributes: the decrypted MSQ body under `--decrypt`, else the raw
    // block. `decoded[i]` records whether decryption was applied; `savegame[i]` flags the
    // savegame block.
    let mut outputs: Vec<Vec<u8>> = Vec::with_capacity(segments.len());
    let mut decoded: Vec<bool> = Vec::with_capacity(segments.len());
    let mut savegame: Vec<bool> = Vec::with_capacity(segments.len());
    for seg in &segments {
        let raw = &bytes[seg.offset..seg.offset + seg.len];
        savegame.push(wasteland::is_savegame_block(raw));
        if decrypt && seg.has_magic {
            let body = wasteland::decrypt(raw)
                .with_context(|| format!("decrypting MSQ block at {:#x}", seg.offset))?;
            outputs.push(body);
            decoded.push(true);
        } else {
            outputs.push(raw.to_vec());
            decoded.push(false);
        }
    }

    let stem = args
        .file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("block");

    // Extract to files when a destination is given.
    let mut written: Vec<Option<PathBuf>> = vec![None; segments.len()];
    if let Some(dir) = &args.out {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        for i in 0..segments.len() {
            let path = dir.join(format!("{stem}_{i:04}.bin"));
            fs::write(&path, &outputs[i]).with_context(|| format!("writing {}", path.display()))?;
            written[i] = Some(path);
        }
    }

    if args.json {
        #[derive(Serialize)]
        struct Block {
            index: usize,
            offset: usize,
            len: usize,
            has_magic: bool,
            decrypted: bool,
            savegame: bool,
            out_len: usize,
            path: Option<String>,
        }
        #[derive(Serialize)]
        struct Output {
            file: String,
            magic: String,
            blocks: Vec<Block>,
        }
        let blocks: Vec<Block> = segments
            .iter()
            .enumerate()
            .map(|(i, s)| Block {
                index: i,
                offset: s.offset,
                len: s.len,
                has_magic: s.has_magic,
                decrypted: decoded[i],
                savegame: savegame[i],
                out_len: outputs[i].len(),
                path: written[i].as_ref().map(|p| p.display().to_string()),
            })
            .collect();
        let json = serde_json::to_string_pretty(&Output {
            file: args.file.display().to_string(),
            magic: util::hex_string(&magic),
            blocks,
        })?;
        println!("{json}");
    } else {
        println!(
            "{} block(s) in {} (magic {}, {} byte(s)){}",
            segments.len(),
            args.file.display(),
            format_magic(&magic),
            magic.len(),
            if decrypt { " [MSQ decrypt]" } else { "" }
        );
        for (i, s) in segments.iter().enumerate() {
            let mut tag = String::new();
            if !s.has_magic {
                tag.push_str("  (preamble)");
            }
            if decoded[i] {
                tag.push_str(&format!("  -> decrypted {:08x}", outputs[i].len()));
            }
            if savegame[i] {
                tag.push_str("  <= savegame");
            }
            println!("  #{i:04}  offset {:08x}  len {:08x}{tag}", s.offset, s.len);
        }
        match &args.out {
            Some(dir) => println!("\nwrote {} file(s) to {}", segments.len(), dir.display()),
            None => println!("\n(dry run — pass --out DIR to extract)"),
        }
    }
    Ok(())
}

/// Show the magic as a quoted ASCII string when fully printable, else as hex.
fn format_magic(magic: &[u8]) -> String {
    if magic.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        format!("\"{}\"", String::from_utf8_lossy(magic))
    } else {
        format!("0x{}", util::hex_string(magic))
    }
}

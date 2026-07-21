//! The **schema explorer**: the mechanical half of mapping an unknown save format. `find` locates
//! a known value (any width/endian) so you can pin a field to an offset; `diff` shows exactly
//! which bytes a before/after pair of saves changed; `stride` measures the spacing between repeated
//! values to reveal record arrays.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use fringe_retro_core::{diff, scan};
use serde::Serialize;

use crate::util;

/// Subcommands under `fringe-retro-kit schema`.
#[derive(Subcommand)]
pub enum Command {
    /// Find every offset where a value is stored (try widths/endianness to pin a field).
    Find(FindArgs),
    /// Show which bytes differ between two saves (a before/after guided diff).
    Diff(DiffArgs),
    /// Measure the spacing between repeated values to reveal a record stride.
    Stride(StrideArgs),
}

/// Integer width of a searched value.
#[derive(Clone, Copy, ValueEnum)]
pub enum Width {
    /// 1 byte.
    Byte,
    /// 2 bytes.
    U16,
    /// 3 bytes.
    U24,
    /// 4 bytes.
    U32,
}

impl Width {
    fn bytes(self) -> usize {
        match self {
            Width::Byte => 1,
            Width::U16 => 2,
            Width::U24 => 3,
            Width::U32 => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Width::Byte => "byte",
            Width::U16 => "u16",
            Width::U24 => "u24",
            Width::U32 => "u32",
        }
    }
}

/// Byte order(s) to search.
#[derive(Clone, Copy, ValueEnum)]
pub enum Endian {
    /// Little-endian only (the DOS/x86 norm).
    Le,
    /// Big-endian only.
    Be,
    /// Try both orders.
    Both,
}

#[derive(Args)]
pub struct FindArgs {
    /// Input file.
    file: PathBuf,
    /// Value to search for (decimal or 0x hex).
    #[arg(long, value_parser = util::u32_arg)]
    value: u32,
    /// Integer width to encode the value as.
    #[arg(long, value_enum, default_value = "u16")]
    width: Width,
    /// Byte order to search.
    #[arg(long, value_enum, default_value = "le")]
    endian: Endian,
    /// Byte offset to start scanning at.
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    offset: usize,
    /// Length of the scanned region (default: to end of file).
    #[arg(long, value_parser = util::usize_arg)]
    len: Option<usize>,
    /// Emit JSON instead of a listing.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct DiffArgs {
    /// The "before" file.
    a: PathBuf,
    /// The "after" file.
    b: PathBuf,
    /// Emit JSON instead of a listing.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct StrideArgs {
    /// Input file.
    file: PathBuf,
    /// Value whose repetitions mark record boundaries (decimal or 0x hex).
    #[arg(long, value_parser = util::u32_arg)]
    value: u32,
    /// Integer width to encode the value as.
    #[arg(long, value_enum, default_value = "u16")]
    width: Width,
    /// Byte order to search.
    #[arg(long, value_enum, default_value = "le")]
    endian: Endian,
    /// Emit JSON instead of a listing.
    #[arg(long)]
    json: bool,
}

/// Dispatch a `schema` subcommand.
pub fn run(command: Command) -> Result<()> {
    match command {
        Command::Find(args) => find(args),
        Command::Diff(args) => diff_files(args),
        Command::Stride(args) => stride(args),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {}", path.display()))
}

/// Little-endian encoding of `value` at `width`, rejecting values that do not fit.
fn encode_le(value: u32, width: Width) -> Result<Vec<u8>> {
    let bytes = width.bytes();
    let max = if bytes >= 4 {
        u32::MAX
    } else {
        (1u32 << (8 * bytes)) - 1
    };
    if value > max {
        bail!(
            "value {value:#x} does not fit in a {} ({bytes}-byte) field",
            width.label()
        );
    }
    Ok((0..bytes).map(|i| (value >> (8 * i)) as u8).collect())
}

/// The needle(s) to search for, each tagged with an endian label. A 1-byte value has a single
/// order regardless of `--endian`.
fn needles(value: u32, width: Width, endian: Endian) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let le = encode_le(value, width)?;
    if le.len() == 1 {
        return Ok(vec![("-", le)]);
    }
    let be: Vec<u8> = le.iter().rev().copied().collect();
    Ok(match endian {
        Endian::Le => vec![("le", le)],
        Endian::Be => vec![("be", be)],
        Endian::Both => vec![("le", le), ("be", be)],
    })
}

/// The `offset..offset+len` slice of `bytes` (to end-of-file when `len` is `None`).
fn region(bytes: &[u8], offset: usize, len: Option<usize>) -> Result<&[u8]> {
    let end = match len {
        Some(l) => offset
            .checked_add(l)
            .ok_or_else(|| anyhow!("offset + len overflows"))?,
        None => bytes.len(),
    };
    bytes.get(offset..end).ok_or_else(|| {
        anyhow!(
            "region {offset}..{end} is out of range (file is {} bytes)",
            bytes.len()
        )
    })
}

fn find(args: FindArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let data = region(&bytes, args.offset, args.len)?;
    let needles = needles(args.value, args.width, args.endian)?;

    // (endian label, absolute offsets).
    let results: Vec<(&'static str, Vec<usize>)> = needles
        .iter()
        .map(|(label, needle)| {
            let hits = scan::find_bytes(data, needle)
                .into_iter()
                .map(|off| args.offset + off)
                .collect();
            (*label, hits)
        })
        .collect();

    if args.json {
        #[derive(Serialize)]
        struct Match {
            endian: &'static str,
            offsets: Vec<usize>,
        }
        #[derive(Serialize)]
        struct Output {
            value: u32,
            width: &'static str,
            results: Vec<Match>,
        }
        let json = serde_json::to_string_pretty(&Output {
            value: args.value,
            width: args.width.label(),
            results: results
                .into_iter()
                .map(|(endian, offsets)| Match { endian, offsets })
                .collect(),
        })?;
        println!("{json}");
    } else {
        for (endian, offsets) in &results {
            let tag = if *endian == "-" {
                String::new()
            } else {
                format!(" {endian}")
            };
            println!(
                "value {:#x} as {}{tag} — {} match(es)",
                args.value,
                args.width.label(),
                offsets.len()
            );
            for off in offsets {
                println!("  {off:08x}");
            }
        }
    }
    Ok(())
}

fn diff_files(args: DiffArgs) -> Result<()> {
    let a = read_file(&args.a)?;
    let b = read_file(&args.b)?;
    let runs = diff::group_runs(&diff::diff_bytes(&a, &b));
    let changed: usize = runs.iter().map(|r| r.old.len()).sum();

    if args.json {
        #[derive(Serialize)]
        struct Run {
            offset: usize,
            old: String,
            new: String,
        }
        #[derive(Serialize)]
        struct Output {
            a_len: usize,
            b_len: usize,
            changed_bytes: usize,
            runs: Vec<Run>,
        }
        let json = serde_json::to_string_pretty(&Output {
            a_len: a.len(),
            b_len: b.len(),
            changed_bytes: changed,
            runs: runs
                .iter()
                .map(|r| Run {
                    offset: r.offset,
                    old: util::hex_string(&r.old),
                    new: util::hex_string(&r.new),
                })
                .collect(),
        })?;
        println!("{json}");
    } else {
        for r in &runs {
            println!(
                "{:08x}  {} -> {}",
                r.offset,
                bytes_hex(&r.old),
                bytes_hex(&r.new)
            );
        }
        println!(
            "\n{} run(s), {} byte(s) changed over {} common byte(s)",
            runs.len(),
            changed,
            a.len().min(b.len())
        );
        if a.len() != b.len() {
            println!(
                "note: files differ in length ({} vs {} bytes)",
                a.len(),
                b.len()
            );
        }
    }
    Ok(())
}

fn stride(args: StrideArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let needles = needles(args.value, args.width, args.endian)?;
    // Search all requested orders and pool the hits; a stride shows up regardless of endianness.
    let mut offsets: Vec<usize> = needles
        .iter()
        .flat_map(|(_, needle)| scan::find_bytes(&bytes, needle))
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    let histogram = scan::gap_histogram(&offsets);

    if args.json {
        #[derive(Serialize)]
        struct Gap {
            delta: usize,
            count: usize,
        }
        #[derive(Serialize)]
        struct Output {
            value: u32,
            width: &'static str,
            matches: usize,
            gaps: Vec<Gap>,
        }
        let json = serde_json::to_string_pretty(&Output {
            value: args.value,
            width: args.width.label(),
            matches: offsets.len(),
            gaps: histogram
                .iter()
                .map(|&(delta, count)| Gap { delta, count })
                .collect(),
        })?;
        println!("{json}");
    } else {
        println!(
            "value {:#x} as {} — {} match(es)",
            args.value,
            args.width.label(),
            offsets.len()
        );
        if histogram.is_empty() {
            println!("(need at least two matches to measure a stride)");
        } else {
            println!("gap histogram (most common first):");
            for (i, (delta, count)) in histogram.iter().enumerate() {
                let mark = if i == 0 { "  <= likely stride" } else { "" };
                println!("  {delta} (0x{delta:x}) x{count}{mark}");
            }
        }
    }
    Ok(())
}

/// Space-separated lowercase hex of a byte slice (for the diff listing).
fn bytes_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

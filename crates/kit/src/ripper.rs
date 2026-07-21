//! The **string ripper**: pull human-readable text out of opaque game files, either as plain
//! printable ASCII (classic `strings`) or as Wasteland's 5-bit packed strings.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use fringe_retro_core::codec::{strings, strings5};
use serde::Serialize;

use crate::util;

/// Subcommands under `fringe-retro-kit strings`.
#[derive(Subcommand)]
pub enum Command {
    /// Extract runs of printable ASCII (classic `strings`).
    Ascii(AsciiArgs),
    /// Decode Wasteland's 5-bit packed strings from an explicit char table and offset.
    FiveBit(FiveBitArgs),
}

/// Arguments for the ASCII ripper.
#[derive(Args)]
pub struct AsciiArgs {
    /// Input file.
    file: PathBuf,
    /// Minimum run length to report.
    #[arg(long, value_parser = util::usize_arg, default_value = "4")]
    min: usize,
    /// Byte offset to start scanning at (decimal or 0x hex).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    offset: usize,
    /// Length of the scanned region (default: to end of file).
    #[arg(long, value_parser = util::usize_arg)]
    len: Option<usize>,
    /// Emit JSON instead of a `offset  text` listing.
    #[arg(long)]
    json: bool,
}

/// Arguments for the 5-bit ripper.
#[derive(Args)]
pub struct FiveBitArgs {
    /// Input file.
    file: PathBuf,
    /// Byte offset of the 60-byte character table (decimal or 0x hex).
    #[arg(long, value_parser = util::usize_arg)]
    char_table: usize,
    /// Byte offset where the packed 5-bit stream begins.
    #[arg(long, value_parser = util::usize_arg)]
    start: usize,
    /// Number of consecutive strings to decode.
    #[arg(long, value_parser = util::usize_arg, default_value = "1")]
    count: usize,
    /// Emit JSON (an array of strings) instead of one string per line.
    #[arg(long)]
    json: bool,
}

/// Dispatch a `strings` subcommand.
pub fn run(command: Command) -> Result<()> {
    match command {
        Command::Ascii(args) => ascii(args),
        Command::FiveBit(args) => five_bit(args),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {}", path.display()))
}

fn ascii(args: AsciiArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let end = match args.len {
        Some(l) => args
            .offset
            .checked_add(l)
            .ok_or_else(|| anyhow!("offset + len overflows"))?,
        None => bytes.len(),
    };
    let region = bytes.get(args.offset..end).ok_or_else(|| {
        anyhow!(
            "region {}..{end} is out of range (file is {} bytes)",
            args.offset,
            bytes.len()
        )
    })?;
    let found = strings::ascii(region, args.min);

    if args.json {
        #[derive(Serialize)]
        struct Row {
            offset: usize,
            text: String,
        }
        let rows: Vec<Row> = found
            .into_iter()
            .map(|f| Row {
                offset: args.offset + f.offset,
                text: f.text,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for f in &found {
            println!("{:08x}  {}", args.offset + f.offset, f.text);
        }
    }
    Ok(())
}

fn five_bit(args: FiveBitArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let table_end = args
        .char_table
        .checked_add(60)
        .ok_or_else(|| anyhow!("char-table offset overflows"))?;
    let char_table = bytes.get(args.char_table..table_end).ok_or_else(|| {
        anyhow!(
            "char table {:#x}..{table_end:#x} is out of range (file is {} bytes)",
            args.char_table,
            bytes.len()
        )
    })?;

    let mut reader = strings5::BitReader::new(&bytes, args.start);
    let mut out = Vec::new();
    for _ in 0..args.count {
        if reader.exhausted() {
            break;
        }
        out.push(strings5::decode_string(char_table, &mut reader));
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for s in &out {
            println!("{s}");
        }
    }
    Ok(())
}

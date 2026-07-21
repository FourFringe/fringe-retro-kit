//! The **codec workbench**: decode encoded blobs, verify round-trips, and identify checksums —
//! the reverse-engineering steps we did by hand for Wasteland (unpack `WL.EXE`, decompress a map,
//! decrypt a block, work out the checksum), packaged as repeatable, scriptable commands.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use fringe_retro_core::codec;
use serde::Serialize;

use crate::util::{self, hex_string, hexdump};

/// Subcommands under `fringe-retro-kit codec`.
#[derive(Subcommand)]
pub enum Command {
    /// List the available codecs and the arguments each one takes.
    List,
    /// Decode a region of a file with a codec and print (or save) the plaintext.
    Decode(DecodeArgs),
    /// Decode then re-encode a region and verify a byte-for-byte round-trip (invertible codecs).
    Roundtrip(DecodeArgs),
    /// Report which checksum algorithm(s) produce an expected value over a region.
    Checksum(ChecksumArgs),
}

/// The codecs the workbench can apply.
#[derive(Clone, Copy, ValueEnum)]
pub enum Codec {
    /// Rolling-XOR stream cipher (Wasteland's MSQ cipher). Symmetric.
    Xor,
    /// Wasteland Huffman decompression.
    Huffman,
    /// EXEPACK executable decompression (operates on the whole file).
    Exepack,
}

impl Codec {
    fn name(self) -> &'static str {
        match self {
            Codec::Xor => "xor",
            Codec::Huffman => "huffman",
            Codec::Exepack => "exepack",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Codec::Xor => "cipher",
            Codec::Huffman | Codec::Exepack => "decompressor",
        }
    }

    /// Whether the codec has an encoder, so `roundtrip` can verify it.
    fn invertible(self) -> bool {
        matches!(self, Codec::Xor)
    }

    fn params(self) -> &'static str {
        match self {
            Codec::Xor => "--seed <byte> [--step <byte>=0x1f] [--offset] [--len]",
            Codec::Huffman => "--count <n> [--offset]",
            Codec::Exepack => "(whole file)",
        }
    }
}

/// Arguments shared by `decode` and `roundtrip`.
#[derive(Args)]
pub struct DecodeArgs {
    /// Input file.
    file: PathBuf,
    /// Codec to apply.
    #[arg(long, value_enum)]
    codec: Codec,
    /// Byte offset where the encoded region begins (decimal or 0x hex).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    offset: usize,
    /// Length of the encoded region (default: to end of file). Ignored by whole-file codecs.
    #[arg(long, value_parser = util::usize_arg)]
    len: Option<usize>,
    /// [xor] Initial key byte (seed).
    #[arg(long, value_parser = util::u8_arg)]
    seed: Option<u8>,
    /// [xor] Amount the key advances (wrapping) after each byte.
    #[arg(long, value_parser = util::u8_arg, default_value = "0x1f")]
    step: u8,
    /// [huffman] Number of decompressed output bytes to produce.
    #[arg(long, value_parser = util::usize_arg)]
    count: Option<usize>,
    /// Write the decoded bytes to this file instead of printing a hex dump.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Emit JSON instead of a hex dump.
    #[arg(long)]
    json: bool,
}

/// Arguments for the checksum solver.
#[derive(Args)]
pub struct ChecksumArgs {
    /// Input file.
    file: PathBuf,
    /// Byte offset of the region to checksum (decimal or 0x hex).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    offset: usize,
    /// Length of the region (default: to end of file).
    #[arg(long, value_parser = util::usize_arg)]
    len: Option<usize>,
    /// The stored/expected checksum; the solver reports which algorithm(s) produce it.
    #[arg(long, value_parser = util::u32_arg)]
    expect: u32,
    /// Emit JSON instead of a table.
    #[arg(long)]
    json: bool,
}

/// Dispatch a `codec` subcommand.
pub fn run(command: Command) -> Result<()> {
    match command {
        Command::List => {
            list();
            Ok(())
        }
        Command::Decode(args) => decode(args),
        Command::Roundtrip(args) => roundtrip(args),
        Command::Checksum(args) => checksum(args),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {}", path.display()))
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

/// Apply the requested codec to `bytes`, returning the decoded plaintext.
fn decode_bytes(bytes: &[u8], args: &DecodeArgs) -> Result<Vec<u8>> {
    match args.codec {
        Codec::Xor => {
            let seed = args
                .seed
                .ok_or_else(|| anyhow!("the xor codec requires --seed"))?;
            Ok(codec::xor::rolling(
                region(bytes, args.offset, args.len)?,
                seed,
                args.step,
            ))
        }
        Codec::Huffman => {
            let count = args
                .count
                .ok_or_else(|| anyhow!("the huffman codec requires --count (output size)"))?;
            let (out, _end) = codec::huffman::decompress(bytes, args.offset, count)?;
            Ok(out)
        }
        Codec::Exepack => Ok(codec::exepack::unpack(bytes)?),
    }
}

fn list() {
    println!(
        "{:<9} {:<13} {:<11} arguments",
        "codec", "kind", "invertible"
    );
    for c in [Codec::Xor, Codec::Huffman, Codec::Exepack] {
        println!(
            "{:<9} {:<13} {:<11} {}",
            c.name(),
            c.kind(),
            if c.invertible() { "yes" } else { "no" },
            c.params(),
        );
    }
}

fn decode(args: DecodeArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let out = decode_bytes(&bytes, &args)?;

    if let Some(path) = &args.out {
        fs::write(path, &out).with_context(|| format!("writing {}", path.display()))?;
        eprintln!("wrote {} bytes to {}", out.len(), path.display());
    } else if args.json {
        #[derive(Serialize)]
        struct Output {
            codec: &'static str,
            file: String,
            output_len: usize,
            hex: String,
        }
        let json = serde_json::to_string_pretty(&Output {
            codec: args.codec.name(),
            file: args.file.display().to_string(),
            output_len: out.len(),
            hex: hex_string(&out),
        })?;
        println!("{json}");
    } else {
        print!("{}", hexdump(&out));
    }
    Ok(())
}

fn roundtrip(args: DecodeArgs) -> Result<()> {
    if !args.codec.invertible() {
        bail!(
            "the {} codec is decode-only; round-trip needs an invertible codec (e.g. xor)",
            args.codec.name()
        );
    }
    let bytes = read_file(&args.file)?;
    let original = region(&bytes, args.offset, args.len)?.to_vec();
    let decoded = decode_bytes(&bytes, &args)?;
    // For a symmetric codec, re-encoding is the same transform applied to the decoded bytes.
    let reencoded = match args.codec {
        Codec::Xor => codec::xor::rolling(
            &decoded,
            args.seed
                .ok_or_else(|| anyhow!("the xor codec requires --seed"))?,
            args.step,
        ),
        Codec::Huffman | Codec::Exepack => unreachable!("guarded by invertible()"),
    };
    let ok = reencoded == original;

    if args.json {
        #[derive(Serialize)]
        struct Output {
            codec: &'static str,
            region_len: usize,
            round_trips: bool,
        }
        let json = serde_json::to_string_pretty(&Output {
            codec: args.codec.name(),
            region_len: original.len(),
            round_trips: ok,
        })?;
        println!("{json}");
    } else {
        println!(
            "{}: round-trip {} ({} bytes)",
            args.codec.name(),
            if ok { "OK" } else { "FAILED" },
            original.len()
        );
    }
    if !ok {
        bail!("round-trip mismatch — the codec did not reproduce the original bytes");
    }
    Ok(())
}

fn checksum(args: ChecksumArgs) -> Result<()> {
    let bytes = read_file(&args.file)?;
    let data = region(&bytes, args.offset, args.len)?;
    let matches = codec::checksum::solve(data, args.expect);

    if args.json {
        #[derive(Serialize)]
        struct Row {
            algorithm: &'static str,
            value: u32,
            matches: bool,
        }
        #[derive(Serialize)]
        struct Output {
            expect: u32,
            region_len: usize,
            matches: Vec<&'static str>,
            algorithms: Vec<Row>,
        }
        let algorithms = codec::checksum::ALGORITHMS
            .iter()
            .map(|&(algorithm, f)| {
                let value = f(data);
                Row {
                    algorithm,
                    value,
                    matches: value == args.expect,
                }
            })
            .collect();
        let json = serde_json::to_string_pretty(&Output {
            expect: args.expect,
            region_len: data.len(),
            matches,
            algorithms,
        })?;
        println!("{json}");
    } else {
        println!("{:<18} {:<10} match", "algorithm", "value");
        for &(algorithm, f) in codec::checksum::ALGORITHMS {
            let value = f(data);
            let mark = if value == args.expect {
                "<= matches"
            } else {
                ""
            };
            println!("{algorithm:<18} {value:#010x} {mark}");
        }
        if matches.is_empty() {
            println!(
                "\nNo known algorithm produces {:#x} over {} bytes.",
                args.expect,
                data.len()
            );
        } else {
            println!("\nMatches: {}", matches.join(", "));
        }
    }
    Ok(())
}

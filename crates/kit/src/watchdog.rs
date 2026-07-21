//! The live **watch/logger**: poll a save file and report which bytes change as the game runs.
//! This is the core reverse-engineering feedback loop — do a thing in-game, watch the offset move
//! — reusing the same byte diff the schema explorer's `diff` uses, but continuously.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Args;
use fringe_retro_core::diff::{self, ChangeRun};
use serde::Serialize;

use crate::util;

#[derive(Args)]
pub struct WatchArgs {
    /// File to watch (e.g. a save the game rewrites).
    file: PathBuf,
    /// Poll interval in milliseconds.
    #[arg(long, value_parser = util::usize_arg, default_value = "500")]
    interval: usize,
    /// Byte offset of the watched region (decimal or 0x hex).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    offset: usize,
    /// Length of the watched region (default: to end of file).
    #[arg(long, value_parser = util::usize_arg)]
    len: Option<usize>,
    /// Stop after this many change events (0 = run until interrupted).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    exit_after: usize,
    /// Stop after this many milliseconds of total run time (0 = no time limit).
    #[arg(long, value_parser = util::usize_arg, default_value = "0")]
    timeout_ms: usize,
    /// Emit one compact JSON object per change instead of a text log.
    #[arg(long)]
    json: bool,
}

/// Tracks the last-seen snapshot of the watched region and reports changes against it.
struct Watcher {
    last: Vec<u8>,
}

impl Watcher {
    fn new(initial: Vec<u8>) -> Self {
        Watcher { last: initial }
    }

    /// If `current` differs from the last snapshot, return the changed runs and adopt `current`
    /// as the new baseline; otherwise return `None`.
    fn poll(&mut self, current: Vec<u8>) -> Option<Vec<ChangeRun>> {
        if current == self.last {
            return None;
        }
        let runs = diff::group_runs(&diff::diff_bytes(&self.last, &current));
        self.last = current;
        Some(runs)
    }
}

/// The `offset..offset+len` slice of `bytes`, clamped to what is actually present (a save being
/// rewritten may momentarily be short), or empty if `offset` is past the end.
fn region(bytes: &[u8], offset: usize, len: Option<usize>) -> &[u8] {
    if offset >= bytes.len() {
        return &[];
    }
    let end = match len {
        Some(l) => offset.saturating_add(l).min(bytes.len()),
        None => bytes.len(),
    };
    &bytes[offset..end]
}

pub fn run(args: WatchArgs) -> Result<()> {
    let initial =
        fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    if args.offset > initial.len() {
        bail!(
            "offset {:#x} is past the end of {} ({} bytes)",
            args.offset,
            args.file.display(),
            initial.len()
        );
    }
    let mut watcher = Watcher::new(region(&initial, args.offset, args.len).to_vec());

    eprintln!(
        "watching {} (every {} ms){}{} — Ctrl-C to stop",
        args.file.display(),
        args.interval,
        if args.exit_after > 0 {
            format!(", stop after {} change(s)", args.exit_after)
        } else {
            String::new()
        },
        if args.timeout_ms > 0 {
            format!(", stop after {} ms", args.timeout_ms)
        } else {
            String::new()
        },
    );

    let start = Instant::now();
    let mut seq = 0usize;
    loop {
        if args.timeout_ms > 0 && start.elapsed() >= Duration::from_millis(args.timeout_ms as u64) {
            break;
        }
        thread::sleep(Duration::from_millis(args.interval as u64));

        // A read may fail transiently while the game rewrites the file; skip and retry.
        let Ok(bytes) = fs::read(&args.file) else {
            continue;
        };
        let current = region(&bytes, args.offset, args.len).to_vec();
        if let Some(runs) = watcher.poll(current) {
            seq += 1;
            report(&args, seq, start.elapsed(), &runs);
            if args.exit_after > 0 && seq >= args.exit_after {
                break;
            }
        }
    }
    Ok(())
}

/// Print one change event, absolute offsets, as text or compact JSON.
fn report(args: &WatchArgs, seq: usize, elapsed: Duration, runs: &[ChangeRun]) {
    let changed: usize = runs.iter().map(|r| r.old.len()).sum();
    if args.json {
        #[derive(Serialize)]
        struct Run {
            offset: usize,
            old: String,
            new: String,
        }
        #[derive(Serialize)]
        struct Event {
            seq: usize,
            elapsed_ms: u128,
            changed_bytes: usize,
            runs: Vec<Run>,
        }
        let event = Event {
            seq,
            elapsed_ms: elapsed.as_millis(),
            changed_bytes: changed,
            runs: runs
                .iter()
                .map(|r| Run {
                    offset: args.offset + r.offset,
                    old: util::hex_string(&r.old),
                    new: util::hex_string(&r.new),
                })
                .collect(),
        };
        // Compact, one object per line (JSONL) so a log can be streamed and parsed incrementally.
        match serde_json::to_string(&event) {
            Ok(line) => println!("{line}"),
            Err(err) => eprintln!("failed to serialize event: {err}"),
        }
    } else {
        println!(
            "[#{seq} t+{:.2}s] {} run(s), {} byte(s) changed",
            elapsed.as_secs_f64(),
            runs.len(),
            changed
        );
        for r in runs {
            println!(
                "  {:08x}  {} -> {}",
                args.offset + r.offset,
                bytes_hex(&r.old),
                bytes_hex(&r.new)
            );
        }
    }
}

/// Space-separated lowercase hex of a byte slice.
fn bytes_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_reports_changes_and_advances_the_baseline() {
        let mut w = Watcher::new(vec![1, 2, 3, 4]);
        // No change against the initial snapshot.
        assert!(w.poll(vec![1, 2, 3, 4]).is_none());
        // Two separate edits -> two runs.
        let runs = w.poll(vec![9, 2, 3, 8]).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].offset, 0);
        assert_eq!(runs[1].offset, 3);
        // The new bytes are now the baseline.
        assert!(w.poll(vec![9, 2, 3, 8]).is_none());
    }

    #[test]
    fn region_clamps_to_available_bytes() {
        assert_eq!(region(&[1, 2, 3, 4], 1, Some(2)), &[2, 3]);
        // len past the end clamps.
        assert_eq!(region(&[1, 2, 3, 4], 2, Some(10)), &[3, 4]);
        // offset past the end is empty.
        assert_eq!(region(&[1, 2], 5, None), &[] as &[u8]);
    }
}

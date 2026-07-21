//! End-to-end tests for the `fringe-retro-kit` binary, driving it as a user would.

use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;

fn kit() -> Command {
    Command::cargo_bin("fringe-retro-kit").unwrap()
}

fn file_with(bytes: &[u8]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(bytes).unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn lists_the_available_codecs() {
    kit()
        .args(["codec", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("xor"))
        .stdout(predicate::str::contains("huffman"))
        .stdout(predicate::str::contains("exepack"));
}

#[test]
fn checksum_solver_identifies_the_wasteland_algorithm() {
    let f = file_with(&[1, 2, 3]);
    kit()
        .args(["codec", "checksum"])
        .arg(f.path())
        .args(["--expect", "0xFFFA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wasteland_msq"))
        .stdout(predicate::str::contains("Matches:"));
}

#[test]
fn xor_decode_produces_a_hex_dump() {
    let f = file_with(b"hello");
    kit()
        .args(["codec", "decode"])
        .arg(f.path())
        .args(["--codec", "xor", "--seed", "0x00", "--step", "0x00"])
        .assert()
        .success()
        // step 0, seed 0 is the identity transform: bytes are unchanged.
        .stdout(predicate::str::contains("|hello|"));
}

#[test]
fn xor_round_trips() {
    let f = file_with(b"Hello, Wasteland!");
    kit()
        .args(["codec", "roundtrip"])
        .arg(f.path())
        .args(["--codec", "xor", "--seed", "0x42"])
        .assert()
        .success()
        .stdout(predicate::str::contains("round-trip OK"));
}

#[test]
fn round_trip_rejects_decode_only_codecs() {
    let f = file_with(b"anything");
    kit()
        .args(["codec", "roundtrip"])
        .arg(f.path())
        .args(["--codec", "huffman", "--count", "4"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("decode-only"));
}

#[test]
fn decode_emits_json() {
    let f = file_with(b"hi");
    kit()
        .args(["codec", "decode"])
        .arg(f.path())
        .args([
            "--codec", "xor", "--seed", "0x00", "--step", "0x00", "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"output_len\": 2"))
        .stdout(predicate::str::contains("\"hex\": \"6869\""));
}

#[test]
fn ascii_ripper_lists_printable_runs_with_offsets() {
    let f = file_with(b"\x00\x01ALLPICS1\x00hi\x00WORLD!");
    kit()
        .args(["strings", "ascii"])
        .arg(f.path())
        .args(["--min", "3"])
        .assert()
        .success()
        // "hi" is too short for --min 3 and is dropped.
        .stdout(predicate::str::contains("00000002  ALLPICS1"))
        .stdout(predicate::str::contains("WORLD!"))
        .stdout(predicate::str::contains("hi").not());
}

#[test]
fn ascii_ripper_emits_json_with_absolute_offsets() {
    let f = file_with(b"\x00\x00\x00\x00ROM");
    kit()
        .args(["strings", "ascii"])
        .arg(f.path())
        .args(["--min", "3", "--offset", "2", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"offset\": 4"))
        .stdout(predicate::str::contains("\"text\": \"ROM\""));
}

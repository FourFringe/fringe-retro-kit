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

#[test]
fn schema_find_locates_a_value_in_both_orders() {
    // 500 = 0x01f4: little-endian "f4 01" at offset 2, big-endian "01 f4" at offset 6.
    let f = file_with(b"\xAA\xBB\xf4\x01\xCC\xDD\x01\xf4");
    kit()
        .args(["schema", "find"])
        .arg(f.path())
        .args(["--value", "500", "--width", "u16", "--endian", "both"])
        .assert()
        .success()
        .stdout(predicate::str::contains("as u16 le"))
        .stdout(predicate::str::contains("  00000002"))
        .stdout(predicate::str::contains("as u16 be"))
        .stdout(predicate::str::contains("  00000006"));
}

#[test]
fn schema_diff_reports_changed_runs() {
    let a = file_with(b"ABCDEFGH");
    let b = file_with(b"ABxyEFGH");
    kit()
        .args(["schema", "diff"])
        .arg(a.path())
        .arg(b.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("00000002  43 44 -> 78 79"))
        .stdout(predicate::str::contains("1 run(s), 2 byte(s) changed"));
}

#[test]
fn schema_stride_surfaces_the_record_spacing() {
    let f = file_with(b"\x00XX\x00YY\x00ZZ\x00");
    kit()
        .args(["schema", "stride"])
        .arg(f.path())
        .args(["--value", "0", "--width", "byte"])
        .assert()
        .success()
        .stdout(predicate::str::contains("likely stride"))
        .stdout(predicate::str::contains("3 (0x3) x3"));
}

#[test]
fn watch_logs_a_change_then_exits() {
    use std::time::Duration;

    let f = file_with(b"AAAAAAAA");
    let path = f.path().to_path_buf();
    // Rewrite the file shortly after the watcher starts polling.
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        std::fs::write(&path, b"AXXAAAAA").unwrap();
    });

    kit()
        .args(["watch"])
        .arg(f.path())
        // `--timeout-ms` bounds the run so the test can never hang if the change is missed.
        .args([
            "--interval",
            "20",
            "--exit-after",
            "1",
            "--timeout-ms",
            "10000",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 run(s)"))
        .stdout(predicate::str::contains("00000001  41 41 -> 58 58"));

    writer.join().unwrap();
}

#[test]
fn watch_errors_on_missing_file() {
    kit()
        .args(["watch", "/no/such/fringe/file.sav"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("reading"));
}

#[test]
fn carve_lists_blocks_by_magic() {
    // Two "msq" blocks back to back.
    let f = file_with(b"msq0AAAAmsq1BBBB");
    kit()
        .args(["carve"])
        .arg(f.path())
        .args(["--magic", "msq"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 block(s)"))
        .stdout(predicate::str::contains(
            "#0000  offset 00000000  len 00000008",
        ))
        .stdout(predicate::str::contains(
            "#0001  offset 00000008  len 00000008",
        ))
        .stdout(predicate::str::contains("dry run"));
}

#[test]
fn carve_extracts_blocks_to_files() {
    let f = file_with(b"msq0AAAAmsq1BBBB");
    let dir = tempfile::tempdir().unwrap();
    kit()
        .args(["carve"])
        .arg(f.path())
        // Hex magic form, exercising the 0x parser.
        .args(["--magic", "0x6d7371", "--out"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote 2 file(s)"));

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 2);
    // Each carved block begins with the magic.
    for entry in entries {
        let bytes = std::fs::read(entry.path()).unwrap();
        assert!(bytes.starts_with(b"msq"));
    }
}

#[test]
fn carve_decrypt_undoes_the_msq_cipher() {
    // A minimal MSQ block: "msq0" header, seed bytes 00 00 (so the initial rolling-XOR key is 0),
    // then the ciphertext of "HI" (0x48 unchanged, 0x49 ^ 0x1f = 0x56).
    let f = file_with(b"msq0\x00\x00\x48\x56");
    let dir = tempfile::tempdir().unwrap();
    kit()
        .args(["carve"])
        .arg(f.path())
        // --magic defaults to msq under --decrypt.
        .args(["--decrypt", "--out"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[MSQ decrypt]"))
        .stdout(predicate::str::contains("-> decrypted 00000002"));

    // The single extracted block holds the decrypted body "HI".
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(std::fs::read(entries[0].path()).unwrap(), b"HI");
}

#[test]
fn carve_decrypt_rejects_non_msq_magic() {
    let f = file_with(b"fooBAR");
    kit()
        .args(["carve"])
        .arg(f.path())
        .args(["--magic", "foo", "--decrypt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("MSQ"));
}

/// Build a valid Wasteland savegame MSQ block: `msq0` + seed `00 00`, then the rolling-XOR
/// ciphertext of a 0x1200-byte body carrying a valid party order (1..=7) at bytes 1..8.
fn savegame_block() -> Vec<u8> {
    let mut body = vec![0u8; 0x1200];
    for i in 0..7 {
        body[1 + i] = (i + 1) as u8;
    }
    let mut key = 0u8;
    let cipher: Vec<u8> = body
        .iter()
        .map(|&b| {
            let c = b ^ key;
            key = key.wrapping_add(0x1f);
            c
        })
        .collect();
    let mut block = b"msq0\x00\x00".to_vec();
    block.extend(cipher);
    block
}

#[test]
fn carve_marks_the_savegame_block() {
    // A savegame block followed by a small non-savegame block.
    let mut data = savegame_block();
    data.extend_from_slice(b"msq0\x00\x00\x48\x56");
    let f = file_with(&data);
    kit()
        .args(["carve"])
        .arg(f.path())
        .arg("--decrypt")
        .assert()
        .success()
        .stdout(predicate::str::contains("2 block(s)"))
        .stdout(predicate::str::contains("<= savegame"));
}

#[test]
fn carve_savegame_only_isolates_the_party_block() {
    let mut data = savegame_block();
    data.extend_from_slice(b"msq0\x00\x00\x48\x56");
    let f = file_with(&data);
    let dir = tempfile::tempdir().unwrap();
    kit()
        .args(["carve"])
        .arg(f.path())
        .args(["--savegame-only", "--out"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1 block(s)"));

    // Exactly the savegame block is written; its decrypted body carries the party order.
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1);
    let body = std::fs::read(entries[0].path()).unwrap();
    assert_eq!(body.len(), 0x1200);
    assert_eq!(&body[1..8], &[1, 2, 3, 4, 5, 6, 7]);
}

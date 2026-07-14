//! End-to-end tests that drive the compiled `fringe-retro` binary, so the headless CLI is
//! exercised on every OS in CI. These build a synthetic save in a temp directory and use the
//! CLI itself to read and write it — no hardcoded byte offsets, no real game files.

use assert_cmd::Command;
use predicates::prelude::*;

/// A minimal valid Ultima I save: 820 zero bytes. `from_bytes` only checks the length, so the
/// CLI's own `set` commands can populate fields from here.
const ULTIMA1_SAVE_LEN: usize = 820;

/// A `fringe-retro` command with an isolated (empty) config, so tests never pick up a real
/// `config.toml` in the working directory.
fn frk() -> Command {
    let mut cmd = Command::cargo_bin("fringe-retro").unwrap();
    cmd.env(
        "FRINGE_RETRO_CONFIG",
        "fringe-retro-tests-no-such-config.toml",
    );
    cmd
}

/// Write a blank Ultima I save into a fresh temp dir and return both (the dir keeps it alive).
fn blank_ultima1() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("PLAYER1.U1");
    std::fs::write(&path, vec![0u8; ULTIMA1_SAVE_LEN]).unwrap();
    (dir, path)
}

#[test]
fn version_flag_works() {
    frk().arg("--version").assert().success();
}

#[test]
fn set_get_and_inspect_round_trip() {
    let (_dir, path) = blank_ultima1();
    let p = path.to_str().unwrap();

    // Set two fields (each writes a backup first), then read them back.
    frk().args(["set", p, "name", "Enki"]).assert().success();
    frk()
        .args(["set", p, "gold", "500"])
        .assert()
        .success()
        .stdout(predicate::str::contains("gold: 0 -> 500"));
    frk()
        .args(["get", p, "gold"])
        .assert()
        .success()
        .stdout(predicate::str::contains("500"));

    // Inspect shows both edits.
    frk()
        .args(["inspect", p])
        .assert()
        .success()
        .stdout(predicate::str::contains("Enki").and(predicate::str::contains("500")));
}

#[test]
fn unknown_field_is_an_error() {
    let (_dir, path) = blank_ultima1();
    frk()
        .args(["get", path.to_str().unwrap(), "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown field"));
}

#[test]
fn backup_then_list_backups() {
    let (_dir, path) = blank_ultima1();
    let p = path.to_str().unwrap();
    frk().args(["backup", p]).assert().success();
    frk()
        .args(["backups", p])
        .assert()
        .success()
        .stdout(predicate::str::contains(".bak"));
}

#[test]
fn resources_lists_bundled_links() {
    frk()
        .args(["resources", "ultima4"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Ultima IV").and(predicate::str::contains("https://")));
}

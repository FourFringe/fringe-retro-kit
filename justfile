# Fringe Retro Kit task runner (https://github.com/casey/just).
# Install once with `brew install just`, then run e.g. `just check` or `just release 0.2.0`.

# Show the available recipes.
default:
    @just --list

# Format, lint, and test — the same gate CI enforces.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked

# Auto-format the whole workspace.
fmt:
    cargo fmt --all

# Run the test suite.
test:
    cargo test --workspace

# Run the CLI. Pass arguments after `--`, e.g. `just run -- inspect ultima6`.
run *ARGS:
    cargo run -- {{ ARGS }}

# Build an optimized release binary.
build:
    cargo build --release

# Cut a release: bump the workspace version, run the gate, commit, and tag.
# Usage: `just release 0.2.0`. Pushing (which triggers the release workflow) is left to you.
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "Working tree is dirty — commit or stash first." >&2
        exit 1
    fi
    sed -i '' -E 's/^version = "[^"]*"/version = "{{ VERSION }}"/' Cargo.toml
    cargo check --workspace >/dev/null   # refresh Cargo.lock for the new version
    just check
    git add Cargo.toml Cargo.lock
    git commit -m "Release v{{ VERSION }}"
    git tag -a "v{{ VERSION }}" -m "v{{ VERSION }}"
    printf '\nTagged v%s. Push to trigger the release workflow:\n  git push origin main && git push origin v%s\n' "{{ VERSION }}" "{{ VERSION }}"

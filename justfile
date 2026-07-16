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

# Bake the Ultima I world map into the configured export dir (input + output from config.toml).
map-export:
    cargo run -q -p fringe-retro-map -- export --game ultima1

# Serve the exported maps and open them in your browser.
map-serve:
    cargo run -q -p fringe-retro-map -- serve --open

# Export the map(s), then serve them in the browser.
map: map-export map-serve

# Cut a release: bump the workspace version, run the gate, commit, tag, and push.
# Usage: `just release 0.2.0`. The push triggers the GitHub release workflow.
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
    git push origin main
    git push origin "v{{ VERSION }}"
    printf '\nReleased v%s — pushed main and the tag; the release workflow is now running.\n' "{{ VERSION }}"

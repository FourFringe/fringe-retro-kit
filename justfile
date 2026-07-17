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

# Bake a game's world map(s) into the export dir, e.g. `just map-export ultima2` (default: ultima1).
map-export GAME=("ultima1"):
    cargo run -q -p fringe-retro-map -- export --game {{ GAME }}

# Serve all exported maps in your browser (one server spans every baked game).
map-serve:
    cargo run -q -p fringe-retro-map -- serve --open

# Bake maps and serve them: `just map` bakes every game, `just map ultima2` bakes just one.
map GAME=("all"):
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{ GAME }}" = "all" ]; then
        for game in ultima1 ultima2 ultima3 ultima4 ultima5; do
            cargo run -q -p fringe-retro-map -- export --game "$game"
        done
    else
        cargo run -q -p fringe-retro-map -- export --game "{{ GAME }}"
    fi
    cargo run -q -p fringe-retro-map -- serve --open

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

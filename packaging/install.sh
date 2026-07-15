#!/bin/sh
# fringe-retro installer — downloads a released binary from GitHub and installs it.
#
# Quick start:
#   curl -fsSL https://raw.githubusercontent.com/FourFringe/fringe-retro-kit/main/packaging/install.sh | sh
#
# Options (when piping, pass them after `-s --`, e.g. `... | sh -s -- --version v0.2.0`):
#   --version <tag>   Install a specific release tag (default: the latest release).
#   --bin-dir <dir>   Install location (default: ~/.local/bin).
#   -h, --help        Show this help and exit.
#
# Environment overrides: FRINGE_RETRO_VERSION, FRINGE_RETRO_BIN_DIR.
#
# macOS (Apple Silicon + Intel) and Linux (x86_64) binaries are published. Linux is built from
# the same source but is not tested against real save files — please report issues at
#   https://github.com/FourFringe/fringe-retro-kit/issues
# Windows uses the .zip asset from the release page (this script does not install it).
# On other systems, build from source: https://github.com/FourFringe/fringe-retro-kit

set -eu

REPO="FourFringe/fringe-retro-kit"
BIN_NAME="fringe-retro"
VERSION="${FRINGE_RETRO_VERSION:-}"
BIN_DIR="${FRINGE_RETRO_BIN_DIR:-$HOME/.local/bin}"

say()  { printf '%s\n' "$*"; }
warn() { printf '%s\n' "$*" >&2; }
err()  { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
fringe-retro installer

Usage:
  curl -fsSL .../install.sh | sh
  curl -fsSL .../install.sh | sh -s -- [options]

Options:
  --version <tag>   Install a specific release tag (default: the latest release).
  --bin-dir <dir>   Install location (default: ~/.local/bin).
  -h, --help        Show this help and exit.

Environment overrides: FRINGE_RETRO_VERSION, FRINGE_RETRO_BIN_DIR.
EOF
  exit 0
}

# --- Parse arguments --------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --version)    VERSION="${2:-}"; shift 2 ;;
    --version=*)  VERSION="${1#*=}"; shift ;;
    --bin-dir)    BIN_DIR="${2:-}"; shift 2 ;;
    --bin-dir=*)  BIN_DIR="${1#*=}"; shift ;;
    -h|--help)    usage ;;
    *)            err "unknown option: $1 (try --help)" ;;
  esac
done

# --- Detect platform --------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin)
    case "$arch" in
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      x86_64|amd64)  target="x86_64-apple-darwin" ;;
      *) err "unsupported macOS architecture '$arch'." ;;
    esac ;;
  Linux)
    case "$arch" in
      x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
      *) err "unsupported Linux architecture '$arch' — only x86_64 binaries are published.
Build from source instead: https://github.com/$REPO" ;;
    esac ;;
  *) err "unsupported OS '$os' — macOS and Linux (x86_64) binaries are published.
On Windows, download the .zip from the releases page; otherwise build from source:
https://github.com/$REPO" ;;
esac

# --- Pick a downloader ------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
  DL_TOOL="curl"
elif command -v wget >/dev/null 2>&1; then
  DL_TOOL="wget"
else
  err "need either curl or wget to download the release."
fi

download_to() { # download_to <url> <dest>
  if [ "$DL_TOOL" = curl ]; then
    curl -fsSL -o "$2" "$1"
  else
    wget -qO "$2" "$1"
  fi
}

resolve_latest() {
  if [ "$DL_TOOL" = curl ]; then
    # Follow the /releases/latest redirect and read the resulting tag URL.
    url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
      "https://github.com/$REPO/releases/latest")" || return 1
    printf '%s\n' "${url##*/}"
  else
    wget -qO- "https://api.github.com/repos/$REPO/releases/latest" \
      | grep -m1 '"tag_name"' \
      | sed -E 's/.*"tag_name" *: *"([^"]+)".*/\1/'
  fi
}

verify_checksum() { # verify_checksum <asset>; run from the dir holding <asset> + <asset>.sha256
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$1.sha256" >/dev/null
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1.sha256" >/dev/null
  else
    warn "no shasum/sha256sum available — skipping checksum verification."
  fi
}

# --- Resolve version --------------------------------------------------------
if [ -z "$VERSION" ]; then
  say "Resolving the latest release ..."
  VERSION="$(resolve_latest)" || err "could not determine the latest version."
  [ -n "$VERSION" ] || err "could not determine the latest version."
fi
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac

asset="${BIN_NAME}-${VERSION}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$VERSION"

# --- Download, verify, install ---------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $asset ..."
download_to "$base/$asset"        "$tmp/$asset"        || err "download failed: $base/$asset"
download_to "$base/$asset.sha256" "$tmp/$asset.sha256" || err "download failed: $base/$asset.sha256"

say "Verifying checksum ..."
( cd "$tmp" && verify_checksum "$asset" ) || err "checksum verification failed for $asset."

say "Extracting ..."
tar -xzf "$tmp/$asset" -C "$tmp"

binpath="$tmp/${asset%.tar.gz}/$BIN_NAME"
if [ ! -f "$binpath" ]; then
  binpath="$(find "$tmp" -type f -name "$BIN_NAME" | head -n1)"
fi
[ -n "$binpath" ] && [ -f "$binpath" ] || err "binary '$BIN_NAME' not found in archive."

mkdir -p "$BIN_DIR"
if ! install -m 0755 "$binpath" "$BIN_DIR/$BIN_NAME" 2>/dev/null; then
  cp "$binpath" "$BIN_DIR/$BIN_NAME"
  chmod 0755 "$BIN_DIR/$BIN_NAME"
fi

say ""
say "Installed $BIN_NAME $VERSION to $BIN_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    say ""
    say "$BIN_DIR is not on your PATH. Add it, e.g.:"
    say "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc && exec zsh"
    ;;
esac

say "Run '$BIN_NAME --help' to get started."

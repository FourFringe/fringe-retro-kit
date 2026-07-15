#!/usr/bin/env python3
"""Render the Homebrew formula for a release.

Reads the canonical formula and substitutes the version and the two per-architecture
SHA-256 checksums, writing the result to a destination path (the tap's `Formula/` copy).
The source formula doubles as the template — its current version/checksums are overwritten,
so they never need to be kept in sync by hand.

Usage:
    update_formula.py VERSION ARM_SHA256 INTEL_SHA256 SRC DST
"""

import os
import re
import sys


def replace_sha_after(arch: str, sha: str, text: str) -> str:
    """Replace the `sha256 "…"` that follows the download URL for `arch`."""
    pattern = re.compile(
        r'(' + re.escape(arch) + r'\.tar\.gz"\s*\n\s*sha256 ")[0-9a-fA-F]{64}"'
    )
    new_text, count = pattern.subn(r"\g<1>" + sha + '"', text)
    if count != 1:
        sys.exit(f"error: expected exactly one sha256 after the {arch} url, found {count}")
    return new_text


def main() -> None:
    if len(sys.argv) != 6:
        sys.exit(__doc__)
    version, arm_sha, intel_sha, src, dst = sys.argv[1:6]

    with open(src, encoding="utf-8") as f:
        text = f.read()

    text, n = re.subn(r'version "[^"]*"', f'version "{version}"', text, count=1)
    if n != 1:
        sys.exit("error: could not find the version line to update")

    text = replace_sha_after("aarch64-apple-darwin", arm_sha, text)
    text = replace_sha_after("x86_64-apple-darwin", intel_sha, text)

    dst_dir = os.path.dirname(dst)
    if dst_dir:
        os.makedirs(dst_dir, exist_ok=True)
    with open(dst, "w", encoding="utf-8") as f:
        f.write(text)


if __name__ == "__main__":
    main()

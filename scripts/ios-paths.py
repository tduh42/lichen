#!/usr/bin/env python3
"""Whether a pull request changes something the iOS app or its UI tests use: CI runs the five
iOS shards only then (.github/workflows/check.yml, job ios-paths). An Android-only or docs-only
pull request would otherwise hold the five macOS runners the free plan gives, about 45 min,
and every other pull request would queue behind it.

Reads the changed paths, one per line, on stdin (`git diff --name-only --no-renames BASE...HEAD`:
a file moved out of an iOS path counts by its old path too). Prints `ios=true` or `ios=false`
on stdout, for $GITHUB_OUTPUT, and why on stderr. In doubt, true: no path read (an empty or
failed list) runs the shards; a needless run costs time, a missed one lets a broken app in.

Usage: git diff --name-only --no-renames BASE...HEAD | python3 -I scripts/ios-paths.py
"""

import sys

# What the iOS app and its tests are built from: the Rust core and its build inputs, the app,
# what it bundles (assets/, config/), the vectors the core's build reads, the scripts that build
# and run them, and CI itself (this rule included).
PREFIXES = (
    "crates/",
    "apps/ios/",
    "assets/",
    "config/",
    "testdata/",
    ".github/workflows/",
)
FILES = (
    "scripts/ui-tests-ios.sh",
    "scripts/build-ios-core.sh",
    "scripts/ios-paths.py",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "deny.toml",
)


def ios(paths):
    """The paths that reach the iOS app or its tests."""
    return [p for p in paths if p.startswith(PREFIXES) or p in FILES]


def main():
    paths = [line.strip() for line in sys.stdin if line.strip()]
    if not paths:
        print("ios=true")
        print("ios-paths: no changed path read: the iOS shards run (in doubt)", file=sys.stderr)
        return
    hits = ios(paths)
    print(f"ios={'true' if hits else 'false'}")
    print(f"ios-paths: {len(paths)} changed path(s), {len(hits)} the iOS app or its tests use", file=sys.stderr)
    for p in hits[:20]:
        print(f"  {p}", file=sys.stderr)
    if len(hits) > 20:
        print(f"  … and {len(hits) - 20} more", file=sys.stderr)
    if not hits:
        print("ios-paths: the iOS shards are not needed", file=sys.stderr)


if __name__ == "__main__":
    if sys.argv[1:]:
        sys.exit(__doc__)
    main()

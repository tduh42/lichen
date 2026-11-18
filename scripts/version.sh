#!/usr/bin/env bash
# lichen's version, from the VERSION file and the history (CONTRIBUTING.md, "Versions").
#   --version  the release: MAJOR.MINOR.N, N = commits since VERSION last changed (one per
#              merged pull request), e.g. 0.10.3
#   --build    the build number: every commit of the history, always growing (Android
#              versionCode, iOS CFBundleVersion)
# Needs the whole history: a shallow clone would number everything wrong.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
  echo "version.sh: a shallow clone has not the history to number a version (fetch-depth: 0)" >&2
  exit 1
fi
case "${1:-}" in
  --version)
    base="$(tr -d '[:space:]' < VERSION)"
    since="$(git log -1 --format=%H -- VERSION)"
    n="$(git rev-list --count "$since"..HEAD)"
    echo "${base%.*}.$n"
    ;;
  --build)
    git rev-list --count HEAD
    ;;
  *)
    echo "usage: scripts/version.sh --version | --build" >&2
    exit 2
    ;;
esac

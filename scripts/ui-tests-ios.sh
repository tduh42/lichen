#!/usr/bin/env bash
# The iOS UI tests (apps/ios/LichenUITests, docs/testing.md) on one simulator, on the demo
# network: builds the core for the simulator only, generates the project, builds the app and
# its tests, and runs them. The results bundle lands in local/ui-tests/.
#
# Usage: scripts/ui-tests-ios.sh [--lang en|fr] [--dark] [--only <Class>[/<testMethod>]]...
#                                [--device <name or UDID>] [--build-only | --no-build]
#   --lang fr      the app in French (default English)
#   --dark         the app in dark mode
#   --only X       only this class or test (repeatable), e.g. --only DirectChatTests
#   --device D     the simulator (default: $LICHEN_SIMULATOR, else the first available iPhone)
#   --build-only   build, do not run;  --no-build: run what the last build made
set -euo pipefail
cd "$(dirname "$0")/.."
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

lang=en
dark=0
only=()
device="${LICHEN_SIMULATOR:-}"
build=1
run=1
while [ $# -gt 0 ]; do
  case "$1" in
    --lang) lang="$2"; shift 2 ;;
    --dark) dark=1; shift ;;
    --only) only+=("-only-testing:LichenUITests/$2"); shift 2 ;;
    --device) device="$2"; shift 2 ;;
    --build-only) run=0; shift ;;
    --no-build) build=0; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done
case "$lang" in en|fr) ;; *) echo "--lang: en or fr" >&2; exit 2 ;; esac

derived=local/dd-uitests
mkdir -p local/ui-tests

if [ -z "$device" ]; then
  # The first available iPhone, a Pro if there is one.
  device=$(xcrun simctl list devices available -j | python3 -I -c '
import json, sys
phones = [d for runtime, ds in json.load(sys.stdin)["devices"].items() if "iOS" in runtime
          for d in ds if d["name"].startswith("iPhone")]
phones.sort(key=lambda d: ("Pro" not in d["name"], d["name"]))
print(phones[0]["udid"] if phones else "")')
  [ -n "$device" ] || { echo "no iPhone simulator available" >&2; exit 1; }
fi
if [[ "$device" =~ ^[0-9A-F-]{36}$ ]]; then
  destination="id=$device"
else
  destination="platform=iOS Simulator,name=$device"
fi

common=(-project apps/ios/Lichen.xcodeproj -scheme Lichen -sdk iphonesimulator
  -destination "$destination" -derivedDataPath "$derived"
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=)

if [ "$build" = 1 ]; then
  scripts/build-ios-core.sh --sim
  (cd apps/ios && xcodegen generate -q)
  set +e
  xcodebuild "${common[@]}" build-for-testing 2>&1 | grep -E 'error:|warning: .*LichenUITests|\*\* TEST BUILD'
  built=${PIPESTATUS[0]}
  set -e
  [ "$built" = 0 ] || { echo "build failed" >&2; exit 1; }
fi
[ "$run" = 1 ] || exit 0

look="$lang"
if [ "$dark" = 1 ]; then look="$lang-dark"; fi
result="local/ui-tests/$(date +%Y%m%d-%H%M%S)-$look.xcresult"
started=$(date +%s)
set +e
TEST_RUNNER_LICHEN_LANG="$lang" TEST_RUNNER_LICHEN_DARK="$dark" \
  xcodebuild "${common[@]}" test-without-building -parallel-testing-enabled NO \
  -resultBundlePath "$result" ${only[@]+"${only[@]}"} 2>&1 \
  | grep -E --line-buffered "^Test Case .*(passed|failed|skipped)|error: |\*\* TEST|Executed [0-9]+ test"
status=${PIPESTATUS[0]}
set -e
took=$(( $(date +%s) - started ))
echo "UI tests ($look): $((took / 60)) min $((took % 60)) s; results in $result"
exit "$status"

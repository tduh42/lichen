#!/usr/bin/env bash
# The iOS UI tests (apps/ios/LichenUITests, docs/testing.md) on the demo network, on one
# simulator or on clones of it in parallel (--workers): builds the core for the simulator only,
# generates the project, builds the app and its tests, and runs them. The results bundle lands
# in local/ui-tests/.
#
# Usage: scripts/ui-tests-ios.sh [--lang en|fr] [--dark] [--only <Class>[/<testMethod>]]...
#                                [--device <name or UDID>] [--workers N] [--shard K/N] [--warm]
#                                [--build-only | --no-build]
#   --lang fr      the app in French (default English)
#   --dark         the app in dark mode
#   --only X       only this class or test (repeatable), e.g. --only DirectChatTests
#   --device D     the simulator (default: $LICHEN_SIMULATOR, else an iPhone, a Pro if there is
#                  one, on the newest iOS runtime)
#   --workers N    N clones of it, each running whole test classes (D-12); 1, the default, on the
#                  Mac (at most two simulators at a time there) and in CI (clones ran about three
#                  times slower there: CI shares the classes out with --shard instead)
#   --shard K/N    the K-th of N shares of the test classes, balanced by their run times on CI,
#                  the last share also counting the French pass (CI: five runners, 1/5 to 5/5)
#   --warm         boot the simulator (before the build, when there is one, so that its first boot
#                  runs meanwhile; CI builds without it: a first boot slowed a build without the
#                  Rust cache by half) and wait for the end of its first boot before the tests (then
#                  shut it down for clones): the tests start on a warm device, and do once what a
#                  device just booted does slowly the first time (CI: a fresh image's first boot)
#   --build-only   build, do not run;  --no-build: run what the last build made
set -euo pipefail
cd "$(dirname "$0")/.."
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

lang=en
dark=0
only=()
device="${LICHEN_SIMULATOR:-}"
workers=1
shard=
warm=0
build=1
run=1
while [ $# -gt 0 ]; do
  case "$1" in
    --lang) lang="$2"; shift 2 ;;
    --dark) dark=1; shift ;;
    --only) only+=("-only-testing:LichenUITests/$2"); shift 2 ;;
    --device) device="$2"; shift 2 ;;
    --workers) workers="$2"; shift 2 ;;
    --warm) warm=1; shift ;;
    --shard) shard="$2"; shift 2 ;;
    --build-only) run=0; shift ;;
    --no-build) build=0; shift ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done
case "$lang" in en|fr) ;; *) echo "--lang: en or fr" >&2; exit 2 ;; esac
[[ "$workers" =~ ^[1-9][0-9]?$ ]] || { echo "--workers: a number from 1" >&2; exit 2; }
if [ "$workers" -gt 1 ]; then
  parallel=(-parallel-testing-enabled YES -parallel-testing-worker-count "$workers"
    -maximum-parallel-testing-workers "$workers")
else
  parallel=(-parallel-testing-enabled NO)
fi

if [ -n "$shard" ]; then
  [[ "$shard" =~ ^[1-9][0-9]*/[1-9][0-9]*$ ]] || { echo "--shard: K/N" >&2; exit 2; }
  # The test classes, spread over N shares, the longest first into the lightest share: their
  # run times on one simulator of CI's runner (s, the mean of run 44981652467's two green
  # attempts, 2026-11-22, the fresh simulator's preparation left out), 300 for a class not in
  # the list. The last share also runs the French pass (check.yml), counted in its load.
  classes=$(python3 -I - "$shard" apps/ios/LichenUITests <<'PY'
import pathlib, re, sys
k, n = (int(x) for x in sys.argv[1].split("/"))
if not 1 <= k <= n:
    sys.exit("--shard: K from 1 to N")
times = {"ContactsTests": 733, "RadioLinkTests": 723, "LifecycleTests": 691, "DirectChatTests": 670,
         "OnboardingTests": 599, "GroupTests": 567, "ChatNavigationTests": 343, "SettingsTests": 339,
         "MeshCoreTests": 333, "MediaTests": 302, "LocationTests": 252, "AccessibilityAuditTests": 218,
         "StringsTests": 1, "LongChatTests": 1}
french = 380  # the French pass: ChatNavigationTests and StringsTests, with its own start
found = sorted(m for f in pathlib.Path(sys.argv[2]).glob("*.swift")
               for m in re.findall(r"^(?:final )?class (\w+Tests)\b", f.read_text(), re.M))
shares = [[0, []] for _ in range(n)]
shares[-1][0] = french
for c in sorted(found, key=lambda c: (-times.get(c, 300), c)):
    lightest = min(shares, key=lambda s: s[0])
    lightest[0] += times.get(c, 300)
    lightest[1].append(c)
print(" ".join(sorted(shares[k - 1][1])))
PY
)
  [ -n "$classes" ] || { echo "--shard $shard: no test class" >&2; exit 2; }
  echo "Shard $shard: $classes"
  for c in $classes; do only+=("-only-testing:LichenUITests/$c"); done
fi

derived=local/dd-uitests
mkdir -p local/ui-tests

if [ -z "$device" ]; then
  # An iPhone on the newest iOS runtime (an image may have several: the SDK's is the newest), a
  # Pro if there is one.
  device=$(xcrun simctl list devices available -j | python3 -I -c '
import json, re, sys
def version(runtime):
    return tuple(int(n) for n in re.findall(r"\d+", runtime.split("iOS-")[-1]))
phones = [(version(runtime), d) for runtime, ds in json.load(sys.stdin)["devices"].items() if "iOS" in runtime
          for d in ds if d["name"].startswith("iPhone")]
phones.sort(key=lambda p: (tuple(-n for n in p[0]), "Pro" not in p[1]["name"], p[1]["name"]))
print(phones[0][1]["udid"] if phones else "")')
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

if [ "$warm" = 1 ]; then
  udid="$device"
  if ! [[ "$udid" =~ ^[0-9A-F-]{36}$ ]]; then
    udid=$(xcrun simctl list devices available -j | python3 -I -c '
import json, sys
name = sys.argv[1]
print(next((d["udid"] for ds in json.load(sys.stdin)["devices"].values() for d in ds if d["name"] == name), ""))' "$device")
  fi
  # Booted before the build: its first boot runs while the app builds (on the Mac; CI builds
  # with --build-only alone, then boots in the next step: see check.yml).
  if [ "$build" = 1 ]; then xcrun simctl boot "$udid" 2>/dev/null || true; fi
fi

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

if [ "$warm" = 1 ]; then
  # Its first boot to the end (data migration, accessibility). Left up for one simulator, where
  # it goes on settling (the system holds the notification permission back for minutes after
  # a first boot); down for clones, which xcodebuild makes of a shut-down device.
  started=$(date +%s)
  xcrun simctl boot "$udid" 2>/dev/null || true
  xcrun simctl bootstatus "$udid" -b >/dev/null
  if [ "$workers" -gt 1 ]; then xcrun simctl shutdown "$udid"; fi
  echo "Simulator $udid warmed up in $(( $(date +%s) - started )) s"
fi

look="$lang"
if [ "$dark" = 1 ]; then look="$lang-dark"; fi
result="local/ui-tests/$(date +%Y%m%d-%H%M%S)-$look.xcresult"
started=$(date +%s)
set +e
if [ -n "${GITHUB_ACTIONS:-}" ]; then
  # What uses the runner's CPU, a sample a minute (summed up below): the demo runs slower than
  # the wall clock when the runner is short of it.
  (while sleep 60; do
     echo "== $(date +%H:%M:%S)"
     top -l 2 -s 2 -n 12 -o cpu -stats cpu,command | awk '/^%CPU/{n++; next} n==2'
   done) > "${result%.xcresult}.top.log" 2>&1 &
  sampler=$!
  # On CI: all of xcodebuild's output, kept beside the results too; a test that runs past 10 min
  # (15 at most) fails instead of holding the run; a test that fails runs once more (a text the
  # demo's radio loses at its busy hub, a tap the slow runner drops), and every first failure is
  # listed below all the same.
  TEST_RUNNER_LICHEN_LANG="$lang" TEST_RUNNER_LICHEN_DARK="$dark" TEST_RUNNER_LICHEN_FRESH_SIMULATOR="$warm" \
    xcodebuild "${common[@]}" test-without-building "${parallel[@]}" \
    -test-timeouts-enabled YES -default-test-execution-time-allowance 600 -maximum-test-execution-time-allowance 900 \
    -retry-tests-on-failure -test-iterations 2 \
    -resultBundlePath "$result" ${only[@]+"${only[@]}"} 2>&1 | tee "${result%.xcresult}.log"
  status=${PIPESTATUS[0]}
  kill "$sampler" 2>/dev/null || true
  echo "Failed at least once (a retry may have passed):"
  grep -E "^Test Case '.*' failed" "${result%.xcresult}.log" \
    | sed -E "s/^Test Case '-\[LichenUITests\.([^ ]+) ([^]]+)\]' failed/  \1\/\2/" || echo "  none"
  echo "CPU, mean per process over $(grep -c '^==' "${result%.xcresult}.top.log") samples (% of one core):"
  awk '/^==/{n++; next} NF>=2 && $1 ~ /^[0-9.]+$/ {c=$2; for (i=3;i<=NF;i++) c=c" "$i; s[c]+=$1}
       END {for (c in s) printf "%7.1f  %s\n", s[c]/(n?n:1), c}' "${result%.xcresult}.top.log" | sort -rn | head -12 || true
else
  TEST_RUNNER_LICHEN_LANG="$lang" TEST_RUNNER_LICHEN_DARK="$dark" TEST_RUNNER_LICHEN_FRESH_SIMULATOR="$warm" \
    xcodebuild "${common[@]}" test-without-building "${parallel[@]}" \
    -resultBundlePath "$result" ${only[@]+"${only[@]}"} 2>&1 \
    | grep -E -i --line-buffered "^Test Case .*(passed|failed|skipped)|error: |\*\* TEST|Executed [0-9]+ test|Testing failed|test runner|crashed"
  status=${PIPESTATUS[0]}
fi
set -e
took=$(( $(date +%s) - started ))
# On CI, what failed, from the result bundle: the log may hold no test line at all.
if [ "$status" != 0 ] && [ -n "${GITHUB_ACTIONS:-}" ] && [ -d "$result" ]; then
  xcrun xcresulttool get test-results summary --path "$result" 2>/dev/null | python3 -I -c '
import json, sys
try:
    s = json.load(sys.stdin)
except ValueError:
    sys.exit(0)
g = s.get
print("Summary: %s, %d passed, %d failed, %d skipped of %d" % (
    g("result"), g("passedTests", 0), g("failedTests", 0), g("skippedTests", 0), g("totalTestCount", 0)))
for f in g("testFailures", []):
    print("  FAILED %s: %s" % (f.get("testIdentifierString") or f.get("testName"), f.get("failureText", "")[:300]))
' || true
fi
echo "UI tests ($look, $workers worker$([ "$workers" = 1 ] || echo s)): $((took / 60)) min $((took % 60)) s; results in $result"
exit "$status"

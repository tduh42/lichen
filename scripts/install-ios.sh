#!/usr/bin/env bash
# Builds lichen and installs it on an iPhone paired with this Mac, signed by your own team. A free
# Apple ID works (a "Personal Team"): the app then stops opening after 7 days until you run this
# again; README › "On your own iPhone" says what to set up once.
#
# Usage: scripts/install-ios.sh [device]
#   device: a name or identifier from `xcrun devicectl list devices` (default: the only iPhone
#   paired and reachable).
# Needs apps/ios/Local.xcconfig (copied from Local.example.xcconfig) with your team id and an
# identifier of your own, and your Apple ID signed in once in Xcode › Settings › Apple Accounts.
# Run it from a Terminal on the Mac itself: Xcode's signing does not work over SSH.
set -euo pipefail
cd "$(dirname "$0")/.."
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

die() { printf '%s\n' "$*" >&2; exit 1; }

cfg=apps/ios/Local.xcconfig
[ -f "$cfg" ] || die "Copy apps/ios/Local.example.xcconfig to $cfg, then set your team id and an identifier of your own."
team="$(sed -n 's/^[[:space:]]*DEVELOPMENT_TEAM[[:space:]]*=[[:space:]]*\([A-Z0-9]*\).*/\1/p' "$cfg" | tail -n 1)"
bundle="$(sed -n 's/^[[:space:]]*LICHEN_BUNDLE_ID[[:space:]]*=[[:space:]]*\([A-Za-z0-9.-]*\).*/\1/p' "$cfg" | tail -n 1)"
if [ -z "$team" ] || [ "$team" = ABCDE12345 ]; then
  die "Set DEVELOPMENT_TEAM in $cfg: Xcode › Settings › Apple Accounts › your Apple ID › the team's id."
fi
if [ -z "$bundle" ] || [ "$bundle" = org.example.lichen ]; then
  die "Set LICHEN_BUNDLE_ID in $cfg to an identifier of your own (Apple gives each one to the first team that registers it)."
fi
command -v xcodegen >/dev/null || die "Install XcodeGen: brew install xcodegen"

# The iPhone: devicectl's list, as JSON, read by Python.
list="$(mktemp -t lichen-devices)"
trap 'rm -f "$list"' EXIT
xcrun devicectl list devices --json-output "$list" >/dev/null 2>&1 || die "xcrun devicectl failed: is Xcode installed and selected?"
pick="$(python3 -I - "$list" "${1:-}" <<'EOF'
import json, sys
devices = json.load(open(sys.argv[1])).get("result", {}).get("devices", [])
wanted = sys.argv[2]
phones = []
for d in devices:
    hw, props = d.get("hardwareProperties", {}), d.get("deviceProperties", {})
    if hw.get("platform") != "iOS" or hw.get("reality") == "virtual":
        continue
    names = {d.get("identifier"), hw.get("udid"), props.get("name")}
    if wanted and wanted not in names:
        continue
    phones.append((hw.get("udid") or "", props.get("name") or "", d.get("connectionProperties", {}).get("pairingState", "")))
if len(phones) != 1:
    found = ", ".join(f"{n} ({u})" for u, n, _ in phones) or "none"
    sys.exit(f"Connect one iPhone (USB, or the same Wi-Fi once paired) or name it; found: {found}")
udid, name, pairing = phones[0]
if pairing and pairing != "paired":
    sys.exit(f"{name} is not paired with this Mac yet: connect it by USB, unlock it and tap Trust.")
print(udid)
print(name)
EOF
)" || exit 1
udid="$(sed -n 1p <<<"$pick")"
name="$(sed -n 2p <<<"$pick")"
echo "== $name ($udid), team $team, $bundle"

echo "== the core (Rust, iPhone and simulator)"
scripts/build-ios-core.sh
(cd apps/ios && xcodegen generate -q)

echo "== the app (Release, signed by your team)"
dd=local/dd-device
mkdir -p local
xcodebuild -quiet -project apps/ios/Lichen.xcodeproj -scheme Lichen -configuration Release \
  -destination "platform=iOS,id=$udid" -derivedDataPath "$dd" \
  -allowProvisioningUpdates -allowProvisioningDeviceRegistration \
  build
app="$dd/Build/Products/Release-iphoneos/Lichen.app"
[ -d "$app" ] || die "No $app: see xcodebuild's messages above."

echo "== installing on $name"
xcrun devicectl device install app --device "$udid" "$app"

cat <<EOF

lichen is installed on $name. The first time:
- if iOS says "Untrusted Developer": Settings › General › VPN & Device Management › your
  Apple ID › Trust (the phone needs the Internet to check it);
- if iOS asks for Developer Mode: Settings › Privacy & Security › Developer Mode, restart.
With a free Apple ID the app stops opening after 7 days: run this script again (your
messages and identity stay on the phone).
EOF

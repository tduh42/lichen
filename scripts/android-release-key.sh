#!/usr/bin/env bash
# Creates lichen's Android release key, once, and gives it to CI (the repository's Actions
# secrets), which signs every published APK with it. Usage: scripts/android-release-key.sh [owner/repo]
#
# Every APK must carry this same key, or Android refuses it as an update and the user has to
# uninstall, which erases the identity. Keep a copy of the key file and its password somewhere
# safe (a password manager): without them, no later APK can update the installed app.
set -euo pipefail
repo="${1:-tduh42/lichen}"
dir="$HOME/.config/lichen"
ks="$dir/android-release.jks"
service=lichen-android-release

if [ -e "$ks" ]; then
  echo "$ks exists already: left as it is." >&2
  exit 1
fi
mkdir -p "$dir"
chmod 700 "$dir"

# A random password, kept in the login Keychain; never printed.
pw="$(openssl rand -base64 32 | tr -d '/+=' | cut -c1-32)"
security add-generic-password -U -s "$service" -a lichen -w "$pw"

LICHEN_KS_PW="$pw" keytool -genkeypair -keystore "$ks" -storetype PKCS12 -alias lichen \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=lichen" \
  -storepass:env LICHEN_KS_PW -keypass:env LICHEN_KS_PW
chmod 600 "$ks"

base64 -i "$ks" | gh secret set LICHEN_KEYSTORE_B64 -R "$repo"
printf '%s' "$pw" | gh secret set LICHEN_KEYSTORE_PASSWORD -R "$repo"

echo
echo "Key: $ks"
echo "Password: Keychain item \"$service\" (security find-generic-password -s $service -w)"
LICHEN_KS_PW="$pw" keytool -list -v -keystore "$ks" -storepass:env LICHEN_KS_PW | grep 'SHA256:'
echo "Secrets LICHEN_KEYSTORE_B64 and LICHEN_KEYSTORE_PASSWORD set on $repo."
echo "Back up the file and the password now."

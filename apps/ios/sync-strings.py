#!/usr/bin/env python3
"""Merges the strings the Swift compiler extracted (.stringsdata) into Lichen/Localizable.xcstrings.

Command-line builds do not update String Catalogs (Xcode does). Usage, after a build:
    python3 -I apps/ios/sync-strings.py
Existing translations are kept; new keys get none; keys no longer in the code are removed.
Prints the keys that still lack a French translation.
"""
import glob
import json
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
catalog_path = os.path.join(here, "Lichen", "Localizable.xcstrings")
# Xcode's DerivedData, or the repo's local/dd (xcodebuild -derivedDataPath local/dd): the
# most recent build wins.
tail = "Build/Intermediates.noindex/Lichen.build/Debug-iphonesimulator/Lichen.build/Objects-normal/arm64"
derived = glob.glob(os.path.expanduser("~/Library/Developer/Xcode/DerivedData/Lichen-*/" + tail))
derived += glob.glob(os.path.join(here, "..", "..", "local", "dd*", tail))
derived = sorted(derived, key=os.path.getmtime)[-1:]
if not derived:
    sys.exit("build the app first (xcodebuild ... build)")

keys = set()
for d in derived:
    for f in glob.glob(os.path.join(d, "*.stringsdata")):
        data = json.load(open(f))
        for e in data.get("tables", {}).get("Localizable", []):
            keys.add(e["key"])

catalog = json.load(open(catalog_path))
strings = catalog.setdefault("strings", {})
for k in list(strings):
    if k not in keys:
        del strings[k]
for k in keys:
    strings.setdefault(k, {})
catalog["strings"] = dict(sorted(strings.items()))
with open(catalog_path, "w") as f:
    json.dump(catalog, f, ensure_ascii=False, indent=2)
    f.write("\n")
missing = [k for k, v in strings.items() if "fr" not in v.get("localizations", {})]
print(f"{len(strings)} keys, {len(missing)} without French")
for k in missing:
    print("  " + k)

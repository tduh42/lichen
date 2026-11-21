#!/usr/bin/env python3
"""The iOS app's text, checked in its String Catalogs (B-ios-26, as Android's B-and-16): the
typographic apostrophe ’ in English (the keys, the source language, and any English value) and
French, never a straight one. CI runs it from check.sh; StringsTests checks the same in the
compiled tables.

Usage: python3 -I scripts/ios-strings.py --check
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOGS = [ROOT / "apps" / "ios" / "Lichen" / name for name in ("Localizable.xcstrings", "InfoPlist.xcstrings")]


def values(node, lang, path):
    """Every text of `node` (a key's localizations), with where it is."""
    if isinstance(node, dict):
        unit = node.get("stringUnit")
        if isinstance(unit, dict) and isinstance(unit.get("value"), str):
            yield path, unit["value"]
        for k, v in node.items():
            if k != "stringUnit":
                yield from values(v, lang, path + [k])


def main():
    bad = []
    for catalog in CATALOGS:
        strings = json.loads(catalog.read_text(encoding="utf-8"))["strings"]
        for key, entry in strings.items():
            if "'" in key:
                bad.append(f"{catalog.name}: key {key!r}")
            for lang, loc in entry.get("localizations", {}).items():
                for path, text in values(loc, lang, [lang]):
                    if "'" in text:
                        bad.append(f"{catalog.name}: {key!r} [{'/'.join(path)}] {text!r}")
    if bad:
        print("ios-strings: a straight apostrophe where the app's text takes ’ (B-ios-26):")
        for b in bad:
            print("  " + b)
        print("ios-strings: write ’ in the Swift literal (the English key) and in the French")
        return 1
    return 0


if __name__ == "__main__":
    if sys.argv[1:] not in ([], ["--check"]):
        sys.exit(__doc__)
    sys.exit(main())

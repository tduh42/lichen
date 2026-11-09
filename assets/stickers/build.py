#!/usr/bin/env python3
"""Builds the sticker set the apps ship (protocol.md §6.3, ADR-0025): Google's Noto Animated
Emoji, as Lottie animations stored in raw DEFLATE, and a manifest that maps each emoji to its
file. Run again only to change the set:
    python3 -I assets/stickers/build.py
Downloads from Google's font CDN (no key); writes assets/stickers/noto/.
"""
import json
import os
import urllib.request
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "noto")
URL = "https://fonts.gstatic.com/s/e/notoemoji/latest/{}/lottie.json"

# The picker's order: (group, codepoints as Google names them).
SET = [
    ("faces", "1f600 1f603 1f604 1f601 1f606 1f605 1f602 1f923 1f60a 1f607 1f970 1f60d 1f929 1f618 "
              "1f60b 1f61c 1f92a 1f917 1f92d 1f914 1f928 1f610 1f644 1f60f 1f62c 1f60c 1f634 1f924 "
              "1f637 1f922 1f975 1f976 1f974 1f635 1f92f 1f920 1f973 1f60e 1f913 1f615 1f61f 1f62e "
              "1f632 1f633 1f97a 1f622 1f62d 1f631 1f629 1f971 1f624 1f621 1f92c 1f608 1f480 1f4a9 "
              "1f921 1f47b 1f47d 1f916 1f648 1f649 1f64a"),
    ("hands", "1f44b 1f44d 1f44e 1f44f 1f64c 1f64f 1f91d 270c_fe0f 1f44c 1f4aa 1f91e 1f918"),
    ("hearts", "2764_fe0f 1f494 1f496 1f525 2728 1f4af"),
    ("celebrations", "1f389 1f38a 1f381 1f382 1f37e 1f37b"),
    ("outdoors", "26c5 1f327_fe0f 2744_fe0f 1f308 1f30b 1f3d5_fe0f 1f332 1f30a 1f6a8 26a1"),
    ("animals", "1f98a 1f43b 1f40c 1f41d 1f98b"),
    ("signs", "2705 274c 2753 2757 1f440"),
]


def emoji(codepoint: str) -> str:
    return "".join(chr(int(p, 16)) for p in codepoint.split("_"))


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    manifest = []
    for group, points in SET:
        for cp in points.split():
            path = os.path.join(OUT, cp + ".json.deflate")
            if not os.path.exists(path):
                with urllib.request.urlopen(URL.format(cp), timeout=30) as r:
                    data = r.read()
                json.loads(data)  # a Lottie animation, or nothing is written
                c = zlib.compressobj(9, zlib.DEFLATED, -15)
                with open(path, "wb") as f:
                    f.write(c.compress(data) + c.flush())
            manifest.append({"emoji": emoji(cp), "file": cp + ".json.deflate", "group": group})
    with open(os.path.join(OUT, "manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(len(manifest), "stickers in", OUT)


main()

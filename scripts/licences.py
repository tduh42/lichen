#!/usr/bin/env python3
"""The licences both apps show (Settings > About > Licences): assets/licences/licences.json.

What ships in the apps: lichen itself, the vendored Codec2, the sticker set, the libraries
each app links (versions read from apps/ios/project.yml and the Android version catalogue),
the map data's attribution, and every Rust crate the core (lc-ffi) links on iOS and Android,
from `cargo metadata`, with the licence files each crate ships.

Usage: python3 -I scripts/licences.py           # writes the file
       python3 -I scripts/licences.py --check   # fails when the file is not current
CHECK_OFFLINE=1 passes --offline to cargo.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "licences" / "licences.json"
TARGETS = ["aarch64-apple-ios", "aarch64-linux-android"]
ROOT_CRATE = "lc-ffi"
# As scripts/build-ios-core.sh and scripts/build-android-core.sh build it.
FEATURES = "lc-ffi/sqlcipher"
# Files a crate ships its licence and notices in.
LICENCE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|COPYRIGHT|NOTICE|UNLICENSE)([-._].*)?$", re.I)


def sh(*args):
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def read_text(path):
    text = path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
    return text.strip("\n") + "\n"


def ios_version(package):
    """The exactVersion of a Swift package in apps/ios/project.yml."""
    yml = (ROOT / "apps/ios/project.yml").read_text(encoding="utf-8")
    m = re.search(rf"^  {package}:\n(?:    .*\n)*?    exactVersion: ([^\s]+)", yml, re.M)
    if not m:
        sys.exit(f"no exactVersion for {package} in apps/ios/project.yml")
    return m.group(1).strip('"')


def android_version(key):
    """A version from apps/android/gradle/libs.versions.toml."""
    toml = (ROOT / "apps/android/gradle/libs.versions.toml").read_text(encoding="utf-8")
    m = re.search(rf'^{key} = "([^"]+)"', toml, re.M)
    if not m:
        sys.exit(f"no {key} in apps/android/gradle/libs.versions.toml")
    return m.group(1)


def crates(texts, text_ids, folders):
    """The crates lc-ffi links (normal dependencies, either app target, with the features the
    apps' core builds use), with their licences. `folders` gets each crate's folder by name."""
    offline = ["--offline"] if os.environ.get("CHECK_OFFLINE") == "1" else []
    packages = {}
    reached = set()
    for target in TARGETS:
        meta = json.loads(
            sh("cargo", "metadata", "--format-version", "1", "--locked", *offline, "--filter-platform", target,
               "--features", FEATURES)
        )
        by_id = {p["id"]: p for p in meta["packages"]}
        packages.update(by_id)
        # Build tools that compile C into the core (openssl-src) are not normal dependencies:
        # their folders are kept for the components below.
        for p in meta["packages"]:
            folders.setdefault(p["name"], Path(p["manifest_path"]).parent)
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        root = next(p["id"] for p in meta["packages"] if p["name"] == ROOT_CRATE and p["source"] is None)
        todo = [root]
        while todo:
            node = nodes[todo.pop()]
            for dep in node["deps"]:
                normal = any(k["kind"] is None for k in dep["dep_kinds"])
                if normal and dep["pkg"] not in reached:
                    reached.add(dep["pkg"])
                    todo.append(dep["pkg"])
    out = []
    # In name order, so that the texts are numbered the same way on every run.
    ordered = sorted((packages[pid] for pid in reached), key=lambda p: (p["name"].lower(), p["version"]))
    for p in ordered:
        if p["source"] is None:  # our own crates: lichen's licence
            continue
        folder = Path(p["manifest_path"]).parent
        files = sorted(f for f in folder.iterdir() if f.is_file() and LICENCE_FILE.match(f.name))
        if p.get("license_file"):
            extra = folder / p["license_file"]
            if extra.is_file() and extra not in files:
                files.append(extra)
        ids = []
        for f in files:
            text = read_text(f)
            if text not in text_ids:
                text_ids[text] = len(texts)
                texts.append(text)
            ids.append(text_ids[text])
        out.append({
            "name": p["name"],
            "version": p["version"],
            "licence": p.get("license") or "see its files",
            "url": p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}",
            "texts": ids,
        })
    return out


def build():
    texts, text_ids = [], {}

    def text_of(path):
        text = read_text(ROOT / path)
        if text not in text_ids:
            text_ids[text] = len(texts)
            texts.append(text)
        return [text_ids[text]]

    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    both, ios, android = ["ios", "android"], ["ios"], ["android"]
    components = [
        {"name": "lichen", "version": version, "licence": "GPL-3.0-or-later",
         "url": "https://github.com/tduh42/lichen", "platforms": both, "texts": text_of("LICENSE")},
        {"name": "Codec2", "version": "1.2.0", "licence": "LGPL-2.1",
         "url": "https://github.com/drowe67/codec2", "platforms": both,
         "texts": text_of("crates/lc-media/codec2/COPYING")},
        {"name": "Noto Animated Emoji © Google", "version": "", "licence": "CC-BY-4.0",
         "url": "https://creativecommons.org/licenses/by/4.0/", "platforms": both, "texts": []},
        {"name": "Map data © OpenStreetMap contributors", "version": "", "licence": "ODbL-1.0",
         "url": "https://www.openstreetmap.org/copyright", "platforms": both, "texts": []},
        {"name": "OpenFreeMap", "version": "", "licence": "map styles and tiles",
         "url": "https://openfreemap.org/", "platforms": both, "texts": []},
        {"name": "MapLibre Native", "version": ios_version("MapLibre"), "licence": "BSD-2-Clause",
         "url": "https://github.com/maplibre/maplibre-native", "platforms": ios, "texts": []},
        {"name": "MapLibre Native", "version": android_version("maplibre"), "licence": "BSD-2-Clause",
         "url": "https://github.com/maplibre/maplibre-native", "platforms": android, "texts": []},
        {"name": "Lottie", "version": ios_version("Lottie"), "licence": "Apache-2.0",
         "url": "https://github.com/airbnb/lottie-ios", "platforms": ios, "texts": []},
        {"name": "Lottie", "version": android_version("lottie"), "licence": "Apache-2.0",
         "url": "https://github.com/airbnb/lottie-android", "platforms": android, "texts": []},
        {"name": "ZXing", "version": android_version("zxing"), "licence": "Apache-2.0",
         "url": "https://github.com/zxing/zxing", "platforms": android, "texts": []},
        {"name": "JNA", "version": android_version("jna"), "licence": "Apache-2.0 OR LGPL-2.1-or-later",
         "url": "https://github.com/java-native-access/jna", "platforms": android, "texts": []},
        {"name": "AndroidX, Jetpack Compose, CameraX", "version": "", "licence": "Apache-2.0",
         "url": "https://developer.android.com/jetpack/androidx", "platforms": android, "texts": []},
        {"name": "Kotlin coroutines", "version": android_version("coroutines"), "licence": "Apache-2.0",
         "url": "https://github.com/Kotlin/kotlinx.coroutines", "platforms": android, "texts": []},
    ]
    folders = {}
    rust = crates(texts, text_ids, folders)
    # C libraries the crates build in (ADR-0036): SQLCipher inside libsqlite3-sys, on SQLite
    # (public domain); on Android, OpenSSL's libcrypto, built from source by openssl-src.
    sqlite_sys = folders["libsqlite3-sys"]
    cipher_version = re.search(r"#define CIPHER_VERSION_NUMBER (\S+)",
                               (sqlite_sys / "sqlcipher/sqlite3.c").read_text(encoding="utf-8", errors="replace"))
    components.append({
        "name": "SQLCipher", "version": cipher_version.group(1) if cipher_version else "",
        "licence": "BSD-3-Clause", "url": "https://www.zetetic.net/sqlcipher/", "platforms": both,
        "texts": text_of(sqlite_sys / "sqlcipher/LICENSE")})
    openssl = folders.get("openssl-src")
    if openssl is None or not any(c["name"] == "openssl-sys" for c in rust):
        sys.exit("OpenSSL is not in the Android core: it is built with the sqlcipher feature (ADR-0036)")
    components.append({
        "name": "OpenSSL (libcrypto)", "version": openssl.name.rsplit("+", 1)[-1],
        "licence": "Apache-2.0", "url": "https://www.openssl.org/", "platforms": android,
        "texts": text_of(openssl / "openssl/LICENSE.txt")})
    data = {
        "generated_by": "scripts/licences.py (scripts/check.sh checks it is current)",
        "components": components,
        "crates": rust,
        "texts": texts,
    }
    return json.dumps(data, indent=1, ensure_ascii=False) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            sys.exit(f"{OUT.relative_to(ROOT)} is not current: run python3 -I scripts/licences.py")
        print(f"{OUT.relative_to(ROOT)} is current")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    data = json.loads(text)
    print(f"{OUT.relative_to(ROOT)}: {len(data['crates'])} crates, {len(data['texts'])} texts, {len(text) // 1024} KiB")


if __name__ == "__main__":
    main()

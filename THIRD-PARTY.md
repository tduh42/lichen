# Third-party code and content

lichen is licensed under the GNU General Public License, version 3 or later (`LICENSE`). It includes or uses the following, under their own licences.

## In this repository

| What | Where | Licence |
|---|---|---|
| [Codec2](https://github.com/drowe67/codec2) 1.2.0, speech codec (a subset, unmodified), by David Rowe and contributors | `crates/lc-media/codec2/` | LGPL-2.1 (`crates/lc-media/codec2/COPYING`) |
| [Noto Animated Emoji](https://googlefonts.github.io/noto-emoji-animation/), 107 Lottie animations, © Google, compressed, not modified | `assets/stickers/noto/` | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| MeshCore test vectors, copied from oxymesh (data, produced by independent oracles) | `testdata/meshcore/` | MIT |

## Used by the apps

| What | Platform | Licence |
|---|---|---|
| [MapLibre Native](https://github.com/maplibre/maplibre-native) 6.31.0 / 13.7.1 | iOS / Android | BSD-2-Clause |
| [Lottie](https://github.com/airbnb/lottie-ios) 4.6.1 / lottie-compose 6.7.1 | iOS / Android | Apache-2.0 |
| [ZXing](https://github.com/zxing/zxing) core 3.5.4 (QR decoding) | Android | Apache-2.0 |
| [JNA](https://github.com/java-native-access/jna) 5.19.1 | Android | Apache-2.0 |
| AndroidX, Jetpack Compose, Kotlin coroutines | Android | Apache-2.0 |

The Rust crates come from crates.io, pinned in `Cargo.lock`. `cargo deny check licenses` (run by `scripts/check.sh` and CI) accepts only licences that can be combined into a GPLv3 work; the list is in `deny.toml`.

## Services

- **Maps:** styles and tiles from [OpenFreeMap](https://openfreemap.org/), map data © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors (ODbL). Both apps show the attribution on the map.
- **GIFs:** a GIF message carries only a [GIPHY](https://giphy.com/) id; the receiving phone loads the image from GIPHY (directly or over Tor). Nothing from GIPHY is bundled.
- **Internet bridge:** [iroh](https://iroh.computer/) may go through n0's public relays; [Arti](https://gitlab.torproject.org/tpo/core/arti) connects to the Tor network.

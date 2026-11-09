# Stickers

The animated stickers lichen ships (protocol.md §6.3, ADR-0025): a choice of 107 of Google's
[Noto Animated Emoji](https://googlefonts.github.io/noto-emoji-animation/), as Lottie
animations stored in raw DEFLATE (`noto/*.json.deflate`), with `noto/manifest.json` mapping
each emoji to its file and its group in the picker. `build.py` downloads and writes them.

The animations are © Google, licensed under the
[Creative Commons Attribution 4.0 International license](https://creativecommons.org/licenses/by/4.0/)
(CC BY 4.0); they are compressed here, not modified. The apps credit them in Settings › About.

# ADR-0025 — Stickers that ship with the app, GIFs by their GIPHY id

- Status: accepted
- Date: 2026-11-09

## Context

Tanguy asked for GIFs, "maybe using an id from known providers", then whether an API without keys exists. A GIF is 100 KB to several MB: never over LoRa (§13). Google shut the Tenor API down on 2026-06-30; GIPHY and KLIPY need an API key for search (GIPHY's free key allows 100 calls an hour, production keys are paid). GIPHY's media servers do serve a GIF from its id without a key. Off-grid, no provider is reachable at all.

## Decision

- **Stickers** (STICKER 0x0A, §6.3): animated emoji that both phones already have. The apps ship about a hundred of Google's Noto Animated Emoji (CC BY 4.0, credited in Settings) as Lottie animations, vector and sharp at any size; a sticker on the air is its emoji's characters, one fragment. It works off-grid and never fetches anything. A receiver that lacks one shows the plain emoji.
- **GIFs** (GIF 0x0B, §6.3): only GIPHY's id travels, with the shape and the title, one fragment. Users find GIFs where they already do (the GIPHY app, its site, their keyboard) and paste the link; the composer turns it into a GIF. No search inside lichen, so no key.
- **The receiver builds the address** from the provider and the id, host and rendition fixed by the spec: no URL is ever taken from a message, so nobody can make a phone load an address of their choosing (a tracking pixel, a hostile server).
- **Privacy**: loading a GIF shows GIPHY the phone's IP address. With Tor on, GIFs load through it, automatically; without, the receiver taps to load unless they asked for automatic loading. Each card names its source.
- **Rendering**: Lottie (Airbnb's libraries, Apache 2.0) for stickers; the platforms' own decoders (ImageIO, ImageDecoder) for animated WebP and GIF. The animations are stored compressed (raw DEFLATE) in the apps, about 1 MB for the set.

## Consequences

- Two app dependencies (lottie-ios, lottie-compose); none in the Rust core.
- A sticker set grows only with an app update; a newer sender's sticker shows as its emoji on an older phone.
- A GIF needs Internet on the receiving side. Off-grid it is a title until then.
- Rejected: GIF search with a key (Tanguy chose not to manage one; possible later from Settings); KLIPY (a key, and ads); animated WebP stickers (75–80 KB each at 128 px against about 9 KB of compressed Lottie, and blurry when shown large); sending GIF bytes over LoRa, even shrunk (minutes of airtime for a reaction).

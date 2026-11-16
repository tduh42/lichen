# Brand

<img src="lichen-icon.svg" width="96" alt="lichen icon">

The mark ("Chirp", chosen on 2026-11-06): two and a half teeth of a chirp, the shape of a LoRa signal on a spectrogram, ending in a dot, the packet leaving. Stroke 5.5 on a 64 grid, round joins and caps.

| File | Use |
|---|---|
| `lichen-icon.svg` | app icon tile: the mark at 70 % of an ink square, corner radius 22.5 % |
| `lichen-mark.svg` | Signal on dark backgrounds |
| `lichen-mark-light-bg.svg` | dark Signal on light backgrounds |
| `lichen-mark-ink.svg`, `lichen-mark-white.svg` | single colour |
| `reference.dc.html` | the design artboard (sizes, lockups, palette), for reference |

Palette:

| Name | Hex | Use |
|---|---|---|
| Ink | `#141413` | icon background |
| Signal | `#FB923C` | on dark backgrounds |
| Dark signal | `#C2410C` | on light backgrounds (the app's accent) |
| Paper | `#FFFFFF` | light background |

Lockups set "lichen" in Space Grotesk Bold, letter spacing −1 at 36 px. Clear space: the dot's height all around. Never stretch or rotate the chirp.

The iOS icons (`apps/ios/Lichen/Assets.xcassets/AppIcon.appiconset`, default, dark and tinted) are drawn by `scripts/render-app-icon.swift`:

```sh
swift scripts/render-app-icon.swift apps/ios/Lichen/Assets.xcassets/AppIcon.appiconset
```

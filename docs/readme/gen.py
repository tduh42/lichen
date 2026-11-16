#!/usr/bin/env python3
"""The README's diagrams, in light and dark (`<name>-light.svg`, `<name>-dark.svg`).

The interactive page (`docs/how-lichen-works.html`) explains the same things with live
diagrams; GitHub shows a README without scripts, so these are plain SVG, a few animated in
SVG itself. Figures come from `docs/measurements.md`. Regenerate with:

    python3 -I docs/readme/gen.py
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))

THEMES = {
    "dark": dict(
        bg="#1A1A18", s2="#262623", line="#34342F", tx="#F4F4F0", tx2="#ABABA3",
        lora="#FB923C", lora_bg="#3A2A1C", iroh="#60A5FA", iroh_bg="#1D2A3A",
        tor="#A78BFA", tor_bg="#2A2440", ok="#4ADE80", ok_bg="#1B3324", sos="#EF4444",
        act_bg="#F4F4F0", act_fg="#141413",
    ),
    "light": dict(
        bg="#FFFFFF", s2="#EFEFEB", line="#E2E2DC", tx="#141413", tx2="#55554F",
        lora="#C2410C", lora_bg="#FBE9DF", iroh="#1D4ED8", iroh_bg="#E3EAFB",
        tor="#6D28D9", tor_bg="#EEE6FB", ok="#15803D", ok_bg="#E2F2E7", sos="#B91C1C",
        act_bg="#141413", act_fg="#FFFFFF",
    ),
}

SANS = "system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"
MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


class Svg:
    def __init__(self, c, w, h, title):
        self.c, self.w, self.h = c, w, h
        self.parts = [
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img">',
            f"<title>{esc(title)}</title>",
            f'<rect width="{w}" height="{h}" rx="16" fill="{c["bg"]}" stroke="{c["line"]}"/>',
        ]

    def add(self, s):
        self.parts.append(s)

    def text(self, x, y, s, size=13, color="tx", anchor="middle", weight=500, mono=False):
        fam = MONO if mono else SANS
        self.add(
            f'<text x="{x}" y="{y}" font-family="{fam}" font-size="{size}" font-weight="{weight}" '
            f'fill="{self.c[color]}" text-anchor="{anchor}">{esc(s)}</text>'
        )

    def rect(self, x, y, w, h, fill="s2", stroke="line", rx=10, extra=""):
        st = f' stroke="{self.c[stroke]}"' if stroke else ""
        self.add(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{self.c[fill]}"{st} {extra}/>')

    def line(self, x1, y1, x2, y2, color="line", width=2, dash=None, extra=""):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{self.c[color]}" stroke-width="{width}"{d} {extra}/>')

    def arrow(self, x1, y1, x2, y2, color="lora", width=2, dash=None):
        self.line(x1, y1, x2, y2, color, width, dash)
        import math
        a = math.atan2(y2 - y1, x2 - x1)
        p1 = (x2 - 11 * math.cos(a - 0.4), y2 - 11 * math.sin(a - 0.4))
        p2 = (x2 - 11 * math.cos(a + 0.4), y2 - 11 * math.sin(a + 0.4))
        self.add(f'<path d="M{x2} {y2} L{p1[0]:.1f} {p1[1]:.1f} L{p2[0]:.1f} {p2[1]:.1f} Z" fill="{self.c[color]}"/>')

    def save(self, name, theme):
        self.add("</svg>")
        with open(os.path.join(HERE, f"{name}-{theme}.svg"), "w") as f:
            f.write("\n".join(self.parts) + "\n")


def radio_icon(s, x, y, color="lora"):
    s.add(
        f'<path d="M{x - 10} {y + 11} L{x - 4} {y - 11} V{y + 11} L{x + 2} {y - 11} V{y + 11} L{x + 7} {y}" '
        f'fill="none" stroke="{s.c[color]}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>'
    )


def phone_icon(s, x, y):
    s.add(f'<rect x="{x - 9}" y="{y - 15}" width="18" height="30" rx="4" fill="none" stroke="{s.c["tx2"]}" stroke-width="1.6"/>')


# ---------------------------------------------------------------------------------------------

def journey(c):
    s = Svg(c, 760, 230, "A direct message from Alice's phone to Bob's: Bluetooth to a stock companion, LoRa through a stock repeater, back over Bluetooth")
    nodes = [(60, "Alice's phone", "app + Rust core", "phone"), (225, "Companion", "stock MeshCore", "radio"),
             (380, "Repeater", "stock MeshCore", "radio"), (535, "Companion", "stock MeshCore", "radio"),
             (700, "Bob's phone", "app + Rust core", "phone")]
    s.line(92, 100, 193, 100, dash="3 5")
    s.line(257, 100, 348, 100, "lora", dash="6 6")
    s.line(412, 100, 503, 100, "lora", dash="6 6")
    s.line(567, 100, 668, 100, dash="3 5")
    for x, a, b, kind in nodes:
        s.rect(x - 32, 75, 64, 50)
        (phone_icon if kind == "phone" else radio_icon)(s, x, 100)
        s.text(x, 150, a)
        s.text(x, 168, b, 11.5, "tx2", weight=400)
    # A packet going, then its acknowledgement coming back (SVG animation, 7 s loop).
    s.add(
        f'<circle r="7" cy="100" fill="{c["lora"]}"><animate attributeName="cx" dur="7s" repeatCount="indefinite" '
        f'values="60;225;380;535;700;700;700" keyTimes="0;0.15;0.3;0.45;0.6;0.62;1"/>'
        f'<animate attributeName="opacity" dur="7s" repeatCount="indefinite" values="1;1;0;0" keyTimes="0;0.6;0.62;1"/></circle>'
    )
    s.add(
        f'<circle r="6" cy="100" fill="{c["ok"]}" opacity="0"><animate attributeName="cx" dur="7s" repeatCount="indefinite" '
        f'values="700;700;60;60" keyTimes="0;0.62;0.95;1"/>'
        f'<animate attributeName="opacity" dur="7s" repeatCount="indefinite" values="0;0;1;1;0" keyTimes="0;0.61;0.62;0.95;1"/></circle>'
    )
    s.text(380, 205, "RAW_CUSTOM in direct mode: repeaters forward it without reading it; the ACK comes back the same way", 12, "tx2", weight=400)
    return s


def frame(c):
    s = Svg(c, 760, 210, "A lichen frame: a 7-byte header in clear, the ChaCha20 ciphertext, the 16-byte Poly1305 tag; header bytes 2 to 6 form the nonce")
    fields = [("dst", "1 B", 1, "s2"), ("src", "1 B", 1, "s2"), ("tag", "2 B", 2, "s2"), ("kind‖index", "1 B", 2.3, "s2"),
              ("total", "1 B", 1.3, "s2"), ("attempt", "1 B", 1.6, "s2"), ("ciphertext", "1–146 B", 6, "lora_bg"), ("tag", "16 B", 2.8, "ok_bg")]
    total = sum(f[2] for f in fields)
    x, W = 30, 700
    for name, size, w, fill in fields:
        ww = W * w / total - 4
        s.rect(x, 30, ww, 54, fill, None, 8)
        s.text(x + ww / 2, 54, name, 12, mono=True)
        s.text(x + ww / 2, 72, size, 10.5, "tx2", weight=400, mono=True)
        x += ww + 4
    s.text(30, 112, "header: in clear, authenticated (the AAD)", 12, "tx2", "start", 400)
    s.text(730, 112, "the tag is checked before anything is read", 12, "tx2", "end", 400)
    # The nonce: header bytes 2..6 then seven zeros.
    x0, cw = 30, 44
    for i in range(12):
        fill = "lora_bg" if i < 5 else "s2"
        s.rect(x0 + i * cw, 140, cw - 4, 30, fill, None, 6)
        label = ["tag", "tag", "k‖i", "total", "att"][i] if i < 5 else "00"
        s.text(x0 + i * cw + (cw - 4) / 2, 160, label, 10.5, "tx" if i < 5 else "tx2", mono=True)
    s.text(x0 + 12 * cw + 8, 154, "the nonce: header bytes 2–6,", 12, "tx2", "start", 400)
    s.text(x0 + 12 * cw + 8, 170, "never twice under a key", 12, "tx2", "start", 400)
    return s


def fragments(c):
    s = Svg(c, 760, 300, "ACK-on-Error: round 0 sends every fragment, one is lost, the receiver answers with its bitmap, round 1 resends only the missing one")
    s.rect(30, 20, 120, 46); s.text(90, 48, "Sender")
    s.rect(610, 20, 120, 46); s.text(670, 48, "Receiver")
    s.line(90, 66, 90, 285); s.line(670, 66, 670, 285)
    y = 92
    s.text(380, y - 8, "round 0 · attempt 0 · 8 fragments, paced for the repeaters", 12, "tx2", weight=400)
    for i in range(8):
        yy = y + i * 13
        if i == 3:
            s.line(90, yy, 330, yy + 6, "lora", 2)
            s.text(345, yy + 10, "✕", 14, "sos", weight=700)
        else:
            s.arrow(90, yy, 664, yy + 6, "lora", 1.6)
    yb = 205
    s.arrow(670, yb, 96, yb + 10, "ok", 2)
    s.text(380, yb - 6, "ACK · bitmap 11101111", 12, "ok", mono=True)
    yr = 245
    s.arrow(90, yr, 664, yr + 6, "lora", 2)
    s.text(380, yr - 6, "round 1 · attempt 1 · fragment 3 only", 12, "tx2", weight=400)
    s.arrow(670, 270, 96, 280, "ok", 2)
    s.text(380, 266, "ACK · COMPLETE", 12, "ok", mono=True)
    return s


def introduction(c):
    s = Svg(c, 760, 300, "The introduction: INTRO, INTRO_ACK and INTRO_FIN between two phones that never met, then a session")
    s.rect(40, 18, 150, 44); s.text(115, 45, "Alice's phone")
    s.rect(570, 18, 150, 44); s.text(645, 45, "Bob's phone")
    s.line(115, 62, 115, 285); s.line(645, 62, 645, 285)
    rows = [(88, 1, "INTRO · 57 B", "salt, ephemeral key, return path: nothing secret"),
            (148, -1, "INTRO_ACK · 120 B", "Bob's identity, sealed; a tag only his key can make"),
            (208, 1, "INTRO_FIN · 72 B", "Alice's identity, sealed; her proof")]
    for y, d, name, what in rows:
        if d > 0:
            s.arrow(115, y, 639, y + 18, "lora", 2)
        else:
            s.arrow(645, y, 121, y + 18, "lora", 2)
        s.text(380, y - 4, name, 13, mono=True)
        s.text(380, y + 36, what, 11.5, "tx2", weight=400)
    s.rect(230, 262, 300, 28, "ok_bg", None, 8)
    s.text(380, 281, "a session: one key per direction · 1.8 s of airtime", 12, "ok")
    return s


def messenger(c):
    s = Svg(c, 760, 230, "A message's life: queued while the radio is away, sending once a frame left, then delivered; a reply or a reaction names its message by an 8-byte reference")
    steps = [(110, "Queued", "radio away · cancel", "s2"), (300, "Sending", "frame 1 of 2 left", "lora_bg"),
             (490, "Delivered", "every fragment acknowledged", "ok_bg"), (670, "Expired", "after 24 h", "s2")]
    for i, (x, a, b, fill) in enumerate(steps):
        s.rect(x - 75, 40, 150, 58, fill, None, 12)
        s.text(x, 64, a, 14, weight=600)
        s.text(x, 84, b, 11, "tx2", weight=400)
        if i < 2:
            s.arrow(x + 76, 69, steps[i + 1][0] - 80, 69, "tx2", 1.6)
    s.line(110, 98, 110, 125, "tx2", 1.2, "3 4")
    s.line(110, 125, 670, 125, "tx2", 1.2, "3 4")
    s.arrow(670, 125, 670, 104, "tx2", 1.2)
    s.text(390, 120, "or, never sent", 11, "tx2", weight=400)
    # A reply and a reaction.
    s.rect(60, 150, 330, 60, "s2", None, 14)
    s.text(76, 172, "↩ Camille · “Hi! I'm at the refuge…”", 11.5, "tx2", "start", 400)
    s.text(76, 195, "On my way, see you at the hut", 13.5, "tx", "start")
    s.rect(400, 172, 58, 28, "s2", "line", 14)
    s.text(429, 191, "👍 1", 13)
    s.text(480, 175, "REPLY: ts ‖ ref (8 B) ‖ text", 11.5, "tx2", "start", 400, True)
    s.text(480, 195, "REACTION: ts ‖ ref (8 B) ‖ emoji", 11.5, "tx2", "start", 400, True)
    return s


def groups(c):
    s = Svg(c, 760, 320, "A group around a repeater: a post is flooded twice and signed; a newcomer scans the admin's QR code, asks to join, and gets every key from the admin")
    R = (380, 150)
    members = [(130, 60, "Camille · admin"), (630, 60, "Bob"), (630, 240, "Carol")]
    for x, y, n in members:
        s.line(x, y, *R, "lora", 2, "6 6")
    s.add(f'<circle cx="{R[0]}" cy="{R[1]}" r="26" fill="{c["s2"]}" stroke="{c["line"]}"/>')
    s.text(R[0], R[1] + 4, "repeater", 11, "tx2", weight=400)
    # Two flooded copies, as rings (SVG animation).
    for delay in ("0s", "0.6s"):
        s.add(
            f'<circle cx="{R[0]}" cy="{R[1]}" r="26" fill="none" stroke="{c["lora"]}" stroke-width="1.5">'
            f'<animate attributeName="r" values="26;220" dur="3s" begin="{delay}" repeatCount="indefinite"/>'
            f'<animate attributeName="opacity" values="1;0" dur="3s" begin="{delay}" repeatCount="indefinite"/></circle>'
        )
    for x, y, n in members:
        s.rect(x - 75, y - 24, 150, 48)
        s.text(x, y + 5, n)
    # The newcomer.
    lx, ly = 130, 245
    s.rect(lx - 75, ly - 24, 150, 48, "bg", "lora", 10, 'stroke-dasharray="4 4"')
    s.text(lx, ly - 2, "Léa · scans the QR")
    s.text(lx, ly + 15, "next to Camille", 11, "tx2", weight=400)
    s.arrow(95, ly - 24, 95, 86, "tx2", 1.6)
    s.text(88, 160, "GROUP_JOIN", 11, "tx2", "end", 400, True)
    s.text(88, 175, "token, 16 B", 11, "tx2", "end", 400)
    s.arrow(165, 86, 165, ly - 24, "ok", 1.8)
    s.text(173, 160, "SENDER_KEYS", 11, "ok", "start", 400, True)
    s.text(173, 175, "every key, one bundle", 11, "ok", "start", 400)
    s.text(380, 305, "posts: encrypted once, signed, flooded twice · keys: through the admin, no session between strangers", 11.5, "tx2", weight=400)
    return s


def location(c):
    s = Svg(c, 760, 260, "Location: a position goes out only when the walker drifts more than 50 m from what the receiver extrapolates")
    route = "M 50 210 L 430 210 L 430 50 L 710 50"
    s.add(f'<path d="{route}" fill="none" stroke="{c["line"]}" stroke-width="2"/>')
    for x, y in [(50, 210), (430, 210), (430, 120), (430, 50)]:
        s.add(f'<circle cx="{x}" cy="{y}" r="7" fill="{c["lora"]}"/>')
    s.add(f'<circle cx="600" cy="50" r="7" fill="{c["tx"]}"/>')
    s.add(f'<circle cx="610" cy="50" r="26" fill="none" stroke="{c["tx2"]}" stroke-dasharray="3 4"/>')
    s.text(610, 95, "the receiver's guess, 50 m around it", 11.5, "tx2", weight=400)
    s.text(240, 235, "a straight walk: one position, then nothing", 11.5, "tx2", weight=400)
    s.text(442, 140, "the turn: the guess goes wrong, a new one", 11.5, "tx2", "start", 400)
    s.text(30, 30, "14 bytes · 4 positions for a 20-minute walk with a turn", 12.5, "tx", "start")
    return s


def search(c):
    s = Svg(c, 760, 280, "Search: the 95 % ellipse around a person in SOS without GPS shrinks from minute 3 to minute 15 as three searchers close in")
    cx, cy = 380, 150
    for semi, m, op in [(2472, 3, 0.35), (1143, 9, 0.6), (752, 15, 1)]:
        rx = semi * 0.1
        s.add(
            f'<ellipse cx="{cx - 18}" cy="{cy + 10}" rx="{rx:.0f}" ry="{rx * 0.55:.0f}" fill="none" '
            f'stroke="{c["lora"]}" stroke-dasharray="5 5" opacity="{op}"/>'
        )
        s.text(cx - 18, cy + 10 - rx * 0.55 - 5, f"minute {m}", 11, "lora", weight=400)
    s.add(f'<circle cx="{cx}" cy="{cy}" r="7" fill="{c["sos"]}"/>')
    s.line(cx + 6, cy - 6, 520, 40, "tx2", 1)
    s.text(526, 38, "Julien, SOS, no GPS", 11.5, "tx2", "start", 400)
    for x, y in [(80, 40), (90, 250), (690, 240)]:
        s.add(f'<circle cx="{x}" cy="{y}" r="6" fill="{c["tx"]}"/>')
    s.text(30, 268, "3 searchers, 2–3 km away, walking in, sharing their measures · error 607 m at minute 3, 298 m at minute 15", 11.5, "tx2", "start", 400)
    return s


def media(c):
    s = Svg(c, 760, 250, "What fits the radio: ten seconds of voice, a photo preview and its full thumbnail, a small file, in fragments of 146 bytes")
    rows = [("10 s of voice · Codec2 700C", 6, "875 B"), ("the same in Codec2 1200", 11, "1,500 B"),
            ("photo preview · AVIF 160×120", 3, "409 B"), ("full thumbnail, when asked", 7, "975 B"),
            ("a file · up to 8 KB", 56, "99 s")]
    y = 30
    for label, n, size in rows:
        s.text(30, y + 17, label, 12.5, "tx", "start")
        shown = min(n, 18)
        for i in range(shown):
            s.rect(260 + i * 15, y + 3, 12, 20, "lora_bg", "lora", 3)
        tail = f"{n} fragments · {size}" if n <= 18 else f"… {n} fragments · {size} at CR 4/8"
        s.text(260 + shown * 15 + 8, y + 18, tail, 11, "tx2", "start", 400, True)
        y += 42
    return s


def bridge(c):
    s = Svg(c, 760, 260, "The Internet bridge: one LoRa datagram announces the content's hash; the content comes over iroh, or Tor, and is kept only if its BLAKE3 hash matches")
    s.rect(30, 110, 130, 56); s.text(95, 134, "Camille"); s.text(95, 152, "offers the file", 11, "tx2", weight=400)
    s.rect(600, 110, 130, 56); s.text(665, 134, "You"); s.text(665, 152, "fetch, check", 11, "tx2", weight=400)
    s.add(f'<path d="M160 118 C 280 30, 480 30, 600 118" fill="none" stroke="{c["lora"]}" stroke-width="2" stroke-dasharray="6 6"/>')
    s.text(380, 40, "LoRa · ANNOUNCE · hash, size, name, providers · one fragment", 11.5, "lora", mono=True)
    s.line(160, 140, 600, 140, "iroh", 2.5)
    s.text(380, 132, "iroh · QUIC, peer to peer · 300 kB in 135 ms", 11.5, "iroh", mono=True)
    s.add(f'<path d="M160 160 L270 215 L380 195 L490 225 L600 160" fill="none" stroke="{c["tor"]}" stroke-width="2" stroke-dasharray="6 5"/>')
    for x, y in [(270, 215), (380, 195), (490, 225)]:
        s.add(f'<circle cx="{x}" cy="{y}" r="9" fill="{c["s2"]}" stroke="{c["line"]}"/>')
    s.text(380, 248, "Tor · to an onion service · 300 kB in 3.9 s", 11.5, "tor", mono=True)
    s.rect(610, 180, 110, 26, "ok_bg", None, 8)
    s.text(665, 198, "BLAKE3 ✓", 12, "ok")
    return s


def airtime(c):
    s = Svg(c, 760, 170, "The airtime budget: 360 seconds an hour at 10 % duty in the 869.4–869.65 MHz sub-band, counted by the phone")
    s.text(30, 36, "360 s of airtime per sliding hour (10 % duty, 869.40–869.65 MHz)", 13, "tx", "start")
    W = 700
    s.rect(30, 52, W, 22, "s2", None, 11)
    parts = [(48, "3 voice notes of 15 s", "lora"), (93, "one 8 KB file", "lora"), (15, "40 positions", "lora")]
    x = 30
    for sec, label, col in parts:
        w = W * sec / 360
        s.rect(x, 52, w, 22, "lora_bg", "lora", 4)
        s.text(x + w / 2, 96, label, 11, "tx2", weight=400)
        s.text(x + w / 2, 112, f"{sec} s", 11, "lora", mono=True)
        x += w
    s.text(x + 8, 68, f"{360 - sum(p[0] for p in parts)} s left", 11.5, "tx2", "start", 400, True)
    s.text(30, 148, "A medium that does not fit is refused with the time it would fit; text and positions wait frame by frame.", 11.5, "tx2", "start", 400)
    return s


FIGURES = [journey, frame, fragments, messenger, introduction, groups, location, search, media, bridge, airtime]

if __name__ == "__main__":
    for fig in FIGURES:
        for theme, c in THEMES.items():
            fig(c).save(fig.__name__, theme)
    print(f"{len(FIGURES) * len(THEMES)} files in {HERE}")

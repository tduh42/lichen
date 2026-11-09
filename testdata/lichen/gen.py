#!/usr/bin/env python3
"""lichen content vectors (protocol.md §6.2): message references, REPLY and REACTION.

An independent oracle (Python's hashlib and struct, not the Rust code). Regenerate with:
    python3 -I testdata/lichen/gen.py > testdata/lichen/content.toml
"""
import hashlib
import struct

TEXT, REPLY, REACTION = 0x01, 0x08, 0x09


def ref(author: bytes, plaintext: bytes) -> bytes:
    return hashlib.sha256(b"lichen/1 ref" + author + plaintext).digest()[:8]


def text(ts: int, s: str) -> bytes:
    return bytes([TEXT]) + struct.pack("<I", ts) + s.encode()


def reply(ts: int, target: bytes, s: str) -> bytes:
    return bytes([REPLY]) + struct.pack("<I", ts) + target + s.encode()


def reaction(ts: int, target: bytes, emoji: str) -> bytes:
    return bytes([REACTION]) + struct.pack("<I", ts) + target + emoji.encode()


camille = hashlib.sha256(b"camille").digest()
julien = hashlib.sha256(b"julien").digest()

print("# lichen content vectors (protocol.md §6.2), produced by testdata/lichen/gen.py, an")
print("# independent oracle (Python hashlib). Do not edit by hand: regenerate instead.")
print()

hi = text(1_793_000_000, "Hi! I'm at the refuge.")
refs = [
    ("camille text", camille, hi),
    ("julien same text", julien, hi),
    ("camille later text", camille, text(1_793_000_001, "Hi! I'm at the refuge.")),
]
for name, author, plain in refs:
    print("[[ref]]")
    print(f'name = "{name}"')
    print(f'author = "{author.hex()}"')
    print(f'plaintext = "{plain.hex()}"')
    print(f'ref = "{ref(author, plain).hex()}"')
    print()

target = ref(camille, hi)
replies = [
    ("reply", 1_793_000_060, "On my way, see you at the hut"),
    ("reply in french", 1_793_000_061, "J'arrive, à tout à l'heure"),
]
for name, ts, s in replies:
    print("[[reply]]")
    print(f'name = "{name}"')
    print(f"ts = {ts}")
    print(f'target = "{target.hex()}"')
    print(f'text = "{s}"')
    print(f'datagram = "{reply(ts, target, s).hex()}"')
    print()

reactions = [
    ("thumbs up", 1_793_000_120, "👍"),
    ("heart, later", 1_793_000_180, "❤️"),
    ("family (a long grapheme)", 1_793_000_181, "👨‍👩‍👧‍👦"),
    ("removed", 1_793_000_240, ""),
]
for name, ts, e in reactions:
    print("[[reaction]]")
    print(f'name = "{name}"')
    print(f"ts = {ts}")
    print(f'target = "{target.hex()}"')
    print(f'emoji = "{e}"')
    print(f'datagram = "{reaction(ts, target, e).hex()}"')
    print()

head = bytes([REACTION]) + struct.pack("<I", 1_793_000_300) + target
rhead = bytes([REPLY]) + struct.pack("<I", 1_793_000_300) + target
malformed = [
    ("reply without text", rhead),
    ("reply cut in its reference", rhead[:9]),
    ("reaction cut in its reference", head[:10]),
    ("reaction not UTF-8", head + b"\xff\xfe"),
    ("reaction over 32 bytes", head + ("👍" * 9).encode()),
]
for name, d in malformed:
    print("[[malformed]]")
    print(f'name = "{name}"')
    print(f'datagram = "{d.hex()}"')
    print()

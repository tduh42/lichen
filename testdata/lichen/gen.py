#!/usr/bin/env python3
"""lichen content vectors: message references, REPLY and REACTION (protocol.md §6.2),
group invitations and GROUP_JOIN (§10.5).

An independent oracle (Python's hashlib and struct, not the Rust code). Regenerate with:
    python3 -I testdata/lichen/gen.py > testdata/lichen/content.toml
"""
import base64
import hashlib
import struct

TEXT, REPLY, REACTION, GROUP_JOIN = 0x01, 0x08, 0x09, 0x13


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

print("# lichen content vectors (protocol.md §6.2, §10.5), produced by testdata/lichen/gen.py, an")
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

def invite(admin: bytes, node: bytes, token: bytes, expires: int, admin_name: str, group_name: str) -> str:
    a, g = admin_name.encode()[:32], group_name.encode()[:32]
    raw = admin + node + token + struct.pack("<I", expires) + bytes([len(a)]) + a + bytes([len(g)]) + g
    return "lichen:1:g:" + base64.urlsafe_b64encode(raw).decode().rstrip("=")


camille_node = hashlib.sha256(b"camille node").digest()
token = hashlib.sha256(b"token").digest()[:16]
invites = [
    ("rando vercors", 1_793_000_600, "Camille", "Rando Vercors"),
    ("names in french", 1_793_000_601, "Camille Brès", "Été à l'Écrins"),
    ("no names", 1_793_000_602, "", ""),
]
for name, exp, an, gn in invites:
    print("[[invite]]")
    print(f'name = "{name}"')
    print(f'admin = "{camille.hex()}"')
    print(f'node = "{camille_node.hex()}"')
    print(f'token = "{token.hex()}"')
    print(f"expires = {exp}")
    print(f'admin_name = "{an}"')
    print(f'group_name = "{gn}"')
    print(f'text = "{invite(camille, camille_node, token, exp, an, gn)}"')
    print()

print("[[group_join]]")
print('name = "join"')
print(f'token = "{token.hex()}"')
print(f'datagram = "{(bytes([GROUP_JOIN]) + token).hex()}"')
print()

ok = invite(camille, camille_node, token, 1_793_000_600, "Camille", "Rando Vercors")
bad_invites = [
    ("a contact card, not an invitation", "lichen:1:" + base64.urlsafe_b64encode(camille + camille_node + b"Camille").decode().rstrip("=")),
    ("cut in the token", "lichen:1:g:" + base64.urlsafe_b64encode(camille + camille_node + token[:8]).decode().rstrip("=")),
    ("not base64url", ok[:-3] + "!!!"),
    ("a name longer than 32 bytes", "lichen:1:g:" + base64.urlsafe_b64encode(camille + camille_node + token + struct.pack("<I", 1) + bytes([33]) + b"x" * 33 + bytes([0])).decode().rstrip("=")),
    ("bytes after the group name", ok + "AA"),
]
for name, text in bad_invites:
    print("[[bad_invite]]")
    print(f'name = "{name}"')
    print(f'text = "{text}"')
    print()


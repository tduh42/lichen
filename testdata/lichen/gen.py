#!/usr/bin/env python3
"""lichen content vectors: message references, REPLY and REACTION (protocol.md §6.2), EDIT and
DELETE (§6.4),
GROUP_STATE, SENDER_KEY and SENDER_KEYS (§10.1–10.2), GROUP_MISSING, GROUP_RESEND and GROUP_DIGEST
(§10.6), GROUP_SYNC (§10.7), GROUP_HANDOVER (§10.8, Ed25519 written here from RFC 8032), the key
step (§10.9, HKDF from Python's hmac), group invitations and GROUP_JOIN (§10.5), stickers, GIFs
and GIPHY links (§6.3), small files (§13.2 kinds 2 and 3, the DEFLATE streams from Python's zlib).

An independent oracle (Python's hashlib, hmac and struct, not the Rust code). Regenerate with:
    python3 -I testdata/lichen/gen.py > testdata/lichen/content.toml
"""
import base64
import hashlib
import hmac
import struct
import zlib

TEXT, FILE, REPLY, REACTION, GROUP_STATE, GROUP_JOIN, SENDER_KEYS = 0x01, 0x02, 0x08, 0x09, 0x11, 0x13, 0x14
GROUP_MISSING, GROUP_RESEND, GROUP_SYNC = 0x15, 0x16, 0x17
SENDER_KEY, GROUP_HANDOVER, GROUP_DIGEST = 0x10, 0x18, 0x19
STICKER, GIF = 0x0A, 0x0B
EDIT, DELETE, TIMER = 0x0C, 0x0D, 0x0E


# ----- Ed25519, RFC 8032 §5.1 (MeshCore's private key is the expanded seed: clamped scalar ‖
# prefix, so a signature from a seed is the RFC's) ------------------------------------------
P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
GY = 4 * pow(5, P - 2, P) % P


def _recover_x(y: int, sign: int) -> int:
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * pow(2, (P - 1) // 4, P) % P
    if x % 2 != sign:
        x = P - x
    return x


G = (_recover_x(GY, 0), GY, 1, _recover_x(GY, 0) * GY % P)


def _add(a, b):
    A = (a[1] - a[0]) * (b[1] - b[0]) % P
    B = (a[1] + a[0]) * (b[1] + b[0]) % P
    C = 2 * a[3] * b[3] * D % P
    Dd = 2 * a[2] * b[2] % P
    E, F, Gg, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % P, Gg * H % P, F * Gg % P, E * H % P)


def _mul(s: int, p):
    q = (0, 1, 1, 0)
    while s:
        if s & 1:
            q = _add(q, p)
        p = _add(p, p)
        s >>= 1
    return q


def _encode(p) -> bytes:
    zi = pow(p[2], P - 2, P)
    x, y = p[0] * zi % P, p[1] * zi % P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def _expand(seed: bytes):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def ed_public(seed: bytes) -> bytes:
    return _encode(_mul(_expand(seed)[0], G))


def ed_sign(seed: bytes, msg: bytes) -> bytes:
    a, prefix = _expand(seed)
    pub = _encode(_mul(a, G))
    r = int.from_bytes(hashlib.sha512(prefix + msg).digest(), "little") % L
    rb = _encode(_mul(r, G))
    k = int.from_bytes(hashlib.sha512(rb + pub + msg).digest(), "little") % L
    return rb + ((r + k * a) % L).to_bytes(32, "little")


def hkdf(salt: bytes, ikm: bytes, info: bytes) -> bytes:
    prk = hmac.new(salt, ikm, hashlib.sha256).digest()
    return hmac.new(prk, info + b"\x01", hashlib.sha256).digest()


def _self_check():
    # RFC 8032 §7.1 test 1 and RFC 5869 A.1 (the first 32 bytes).
    sk = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
    assert ed_public(sk).hex() == "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    assert ed_sign(sk, b"").hex() == (
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    )
    okm = hkdf(bytes(range(13)), bytes([0x0B] * 22), bytes(range(0xF0, 0xFA)))
    assert okm.hex() == "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"


_self_check()


def ref(author: bytes, plaintext: bytes) -> bytes:
    return hashlib.sha256(b"lichen/1 ref" + author + plaintext).digest()[:8]


def text(ts: int, s: str) -> bytes:
    return bytes([TEXT]) + struct.pack("<I", ts) + s.encode()


def reply(ts: int, target: bytes, s: str) -> bytes:
    return bytes([REPLY]) + struct.pack("<I", ts) + target + s.encode()


def reaction(ts: int, target: bytes, emoji: str) -> bytes:
    return bytes([REACTION]) + struct.pack("<I", ts) + target + emoji.encode()


def edit(ts: int, target: bytes, s: str) -> bytes:
    return bytes([EDIT]) + struct.pack("<I", ts) + target + s.encode()


def delete(ts: int, target: bytes) -> bytes:
    return bytes([DELETE]) + struct.pack("<I", ts) + target


def timer(ts: int, seconds: int) -> bytes:
    return bytes([TIMER]) + struct.pack("<II", ts, seconds)


camille = hashlib.sha256(b"camille").digest()
julien = hashlib.sha256(b"julien").digest()

print("# lichen content vectors (protocol.md §6.2, §6.3, §6.4, §10.1–10.2, §10.5–10.9, §13.2), produced by testdata/lichen/gen.py, an")
print("# independent oracle (Python hashlib and hmac, Ed25519 from RFC 8032). Do not edit by hand: regenerate instead.")
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

edits = [
    ("edit", 1_793_000_400, "Hi! I'm at the refuge, room 3."),
    ("edit in french", 1_793_000_401, "Salut ! Je suis au refuge, dortoir 3."),
]
for name, ts, s in edits:
    print("[[edit]]")
    print(f'name = "{name}"')
    print(f"ts = {ts}")
    print(f'target = "{target.hex()}"')
    print(f'text = "{s}"')
    print(f'datagram = "{edit(ts, target, s).hex()}"')
    print()

print("[[delete]]")
print('name = "delete"')
print("ts = 1793000500")
print(f'target = "{target.hex()}"')
print(f'datagram = "{delete(1_793_000_500, target).hex()}"')
print()

for name, secs in (("timer off", 0), ("timer 5 minutes", 300), ("timer 1 day", 86_400), ("timer 4 weeks", 28 * 86_400)):
    print("[[timer]]")
    print(f'name = "{name}"')
    print("ts = 1793000600")
    print(f"seconds = {secs}")
    print(f'datagram = "{timer(1_793_000_600, secs).hex()}"')
    print()

head = bytes([REACTION]) + struct.pack("<I", 1_793_000_300) + target
rhead = bytes([REPLY]) + struct.pack("<I", 1_793_000_300) + target
ehead = bytes([EDIT]) + struct.pack("<I", 1_793_000_300) + target
dgram = delete(1_793_000_300, target)
malformed = [
    ("reply without text", rhead),
    ("reply cut in its reference", rhead[:9]),
    ("reaction cut in its reference", head[:10]),
    ("reaction not UTF-8", head + b"\xff\xfe"),
    ("reaction over 32 bytes", head + ("👍" * 9).encode()),
    ("edit without text", ehead),
    ("edit not UTF-8", ehead + b"\xff\xfe"),
    ("edit cut in its reference", ehead[:9]),
    ("delete cut in its reference", dgram[:12]),
    ("delete with trailing bytes", dgram + b"\x00"),
    ("timer cut", timer(1_793_000_600, 300)[:8]),
    ("timer with trailing bytes", timer(1_793_000_600, 300) + b"\x00"),
    ("timer over four weeks", timer(1_793_000_600, 28 * 86_400 + 1)),
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


def name(n: str) -> bytes:
    b = n.encode()[:32]
    return bytes([len(b)]) + b


def group_state(gid: bytes, epoch: int, admin: bytes, gname: str, members, cause=None) -> bytes:
    v = bytes([GROUP_STATE]) + gid + struct.pack("<I", epoch) + admin + name(gname) + bytes([len(members)])
    for ident, node, n in members:
        v += ident + node + name(n)
    if cause is not None:
        v += bytes([cause])
    return v


def sender_keys(gid: bytes, entries) -> bytes:
    v = bytes([SENDER_KEYS]) + gid + bytes([len(entries)])
    for owner, step, epoch, key, nxt in entries:
        v += owner + struct.pack("<I", step) + bytes([epoch]) + key + struct.pack("<I", nxt)
    return v


def sender_key(gid: bytes, step: int, epoch: int, key: bytes, nxt: int) -> bytes:
    return bytes([SENDER_KEY]) + gid + struct.pack("<I", step) + bytes([epoch]) + key + struct.pack("<I", nxt)


gid = hashlib.sha256(b"rando vercors").digest()[:16]
lea = hashlib.sha256(b"lea").digest()
julien_node = hashlib.sha256(b"julien node").digest()
lea_node = hashlib.sha256(b"lea node").digest()
three = [(camille, camille_node, "Camille"), (julien, julien_node, "Julien"), (lea, lea_node, "Léa")]
states = [
    ("three members", 3, "Rando Vercors", three, None),
    ("julien left", 4, "Rando Vercors", [three[0], three[2]], 1),
    ("julien removed", 4, "Rando Vercors", [three[0], three[2]], 2),
    ("renamed", 5, "Vercors, dimanche", [three[0], three[2]], None),
    ("closed", 6, "Vercors, dimanche", [three[0]], None),
]
for nm, epoch, gname, members, cause in states:
    print("[[group_state]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f"epoch = {epoch}")
    print(f'admin = "{camille.hex()}"')
    print(f'group_name = "{gname}"')
    print(f"members = {len(members)}")
    print(f"cause = {cause if cause is not None else -1}")
    print(f'datagram = "{group_state(gid, epoch, camille, gname, members, cause).hex()}"')
    print()

keys = [
    ("one key", [(julien, 3, 1, hashlib.sha256(b"julien key").digest(), 7)]),
    ("the admin's and two members'", [
        (camille, 3, 0, hashlib.sha256(b"camille key").digest(), 0),
        (julien, 3, 1, hashlib.sha256(b"julien key").digest(), 7),
        (lea, 0xFFFFFFFF, 255, hashlib.sha256(b"lea key").digest(), 65536),
    ]),
]
for nm, entries in keys:
    print("[[sender_keys]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f"n = {len(entries)}")
    print(f'owners = "{"".join(o.hex() for o, _, _, _, _ in entries)}"')
    print(f"steps = {[st for _, st, _, _, _ in entries]}")
    print(f'datagram = "{sender_keys(gid, entries).hex()}"')
    print()

def group_missing(gid: bytes, author: bytes, epoch: int, counters) -> bytes:
    return bytes([GROUP_MISSING]) + gid + author + bytes([epoch, len(counters)]) + b"".join(struct.pack("<I", c) for c in counters)


def group_resend(gid: bytes, payloads) -> bytes:
    return bytes([GROUP_RESEND]) + gid + bytes([len(payloads)]) + b"".join(bytes([len(p)]) + p for p in payloads)


missing = [
    ("one counter", julien, 1, [41]),
    ("eight counters, the most", lea, 255, list(range(100, 108))),
]
for nm, author, epoch, counters in missing:
    print("[[group_missing]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f'author = "{author.hex()}"')
    print(f"epoch = {epoch}")
    print(f"counters = {counters}")
    print(f'datagram = "{group_missing(gid, author, epoch, counters).hex()}"')
    print()

# Envelope payloads are opaque here: their own vectors are in testdata/meshcore and §10.3.
env1 = bytes([0x7C, 0x02]) + hashlib.sha256(b"envelope 1").digest() * 3
env2 = bytes([0x7C, 0x02]) + hashlib.sha256(b"envelope 2").digest()[:20]
resend = [
    ("one envelope", [env1]),
    ("a two-fragment message", [env1, env2]),
    ("the longest payload", [bytes([0x7C]) + bytes(155)]),
]
for nm, payloads in resend:
    print("[[group_resend]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f"lengths = {[len(p) for p in payloads]}")
    print(f'datagram = "{group_resend(gid, payloads).hex()}"')
    print()

def group_sync(gid: bytes, epoch: int) -> bytes:
    return bytes([GROUP_SYNC]) + gid + struct.pack("<I", epoch)


for nm, epoch in [("state 3 held", 3), ("the largest epoch", 0xFFFFFFFF)]:
    print("[[group_sync]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f"epoch = {epoch}")
    print(f'datagram = "{group_sync(gid, epoch).hex()}"')
    print()

for nm, step, epoch, key, nxt in [
    ("a first key", 3, 0, hashlib.sha256(b"lea key").digest(), 0),
    ("after a restart, from counter 12", 0xFFFFFFFF, 200, hashlib.sha256(b"julien key 2").digest(), 12),
]:
    print("[[sender_key]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f"step = {step}")
    print(f"key_epoch = {epoch}")
    print(f'key = "{key.hex()}"')
    print(f"next_counter = {nxt}")
    print(f'datagram = "{sender_key(gid, step, epoch, key, nxt).hex()}"')
    print()


def key_step(gid: bytes, key: bytes, epoch: int) -> bytes:
    """The key at state `epoch`, from the key at `epoch − 1` (§10.9)."""
    return hkdf(gid, key, b"lichen/1 key step" + struct.pack("<I", epoch))


k0 = hashlib.sha256(b"julien key").digest()
for nm, start, to in [("one step", 3, 4), ("three steps", 3, 6), ("sixty-four steps, the most", 100, 164)]:
    k = k0
    for e in range(start + 1, to + 1):
        k = key_step(gid, k, e)
    print("[[key_step]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f'key = "{k0.hex()}"')
    print(f"from = {start}")
    print(f"to = {to}")
    print(f'stepped = "{k.hex()}"')
    print()


def group_digest(epoch: int, counter: int) -> bytes:
    return bytes([GROUP_DIGEST, epoch]) + struct.pack("<I", counter)


for nm, epoch, counter in [("after message 41", 1, 41), ("the first message", 0, 0)]:
    print("[[group_digest]]")
    print(f'name = "{nm}"')
    print(f"key_epoch = {epoch}")
    print(f"counter = {counter}")
    print(f'body = "{group_digest(epoch, counter).hex()}"')
    print()

# Handovers (§10.8): real identities, since links are signed.
camille_seed = hashlib.sha256(b"camille seed").digest()
julien_seed = hashlib.sha256(b"julien seed").digest()
camille_id, julien_id, lea_id = ed_public(camille_seed), ed_public(julien_seed), lea
real = [(camille_id, camille_node, "Camille"), (julien_id, julien_node, "Julien"), (lea_id, lea_node, "Léa")]


def link(seed: bytes, state: bytes) -> bytes:
    sig = ed_sign(seed, b"lichen/1 handover" + hashlib.sha256(state).digest())
    return sig + state


def group_handover(links) -> bytes:
    return bytes([GROUP_HANDOVER, len(links)]) + b"".join(struct.pack("<H", len(l) - 64) + l for l in links)


make_admin = group_state(gid, 4, julien_id, "Rando Vercors", real)
leaves = group_state(gid, 4, julien_id, "Rando Vercors", real[1:], 1)
to_lea = group_state(gid, 6, lea_id, "Rando Vercors", real)
handovers = [
    ("camille makes julien admin", [link(camille_seed, make_admin)], [4], [julien_id]),
    ("camille leaves, julien runs the group", [link(camille_seed, leaves)], [4], [julien_id]),
    ("a chain: camille to julien, julien to lea", [link(camille_seed, make_admin), link(julien_seed, to_lea)], [4, 6], [julien_id, lea_id]),
]
for nm, links, epochs, successors in handovers:
    print("[[group_handover]]")
    print(f'name = "{nm}"')
    print(f'gid = "{gid.hex()}"')
    print(f'admin = "{camille_id.hex()}"')
    print(f'admin_seed = "{camille_seed.hex()}"')
    print(f'julien_seed = "{julien_seed.hex()}"')
    print(f"epochs = {epochs}")
    print(f'successors = "{"".join(x.hex() for x in successors)}"')
    print(f'datagram = "{group_handover(links).hex()}"')
    print()

one = group_handover([link(camille_seed, make_admin)])
bad = [
    ("handover, no link", bytes([GROUP_HANDOVER, 0])),
    ("handover, five links", group_handover([link(camille_seed, make_admin)] * 5)),
    ("handover, cut in a link", one[:-1]),
    ("handover, a byte after the links", one + b"\x00"),
    ("handover, a link that is not a state", group_handover([bytes(64) + group_sync(gid, 3)])),
    ("handover, a state with a byte too many", group_handover([link(camille_seed, make_admin + b"\x01\x02")])),
    ("sender key, cut in the counter", sender_key(gid, 3, 0, k0, 0)[:-1]),
    ("sender key, a byte after the counter", sender_key(gid, 3, 0, k0, 0) + b"\x00"),
    ("sync, cut in the epoch", group_sync(gid, 3)[:-1]),
    ("sync, a byte after the epoch", group_sync(gid, 3) + b"\x00"),
    ("missing, no counter", group_missing(gid, julien, 1, [])),
    ("missing, nine counters", group_missing(gid, julien, 1, list(range(9)))),
    ("missing, cut in a counter", group_missing(gid, julien, 1, [41])[:-1]),
    ("resend, no payload", group_resend(gid, [])),
    ("resend, a payload of 160 bytes", group_resend(gid, [bytes(160)])),
    ("resend, an empty payload", group_resend(gid, [b""])),
    ("resend, cut in a payload", group_resend(gid, [env1])[:-1]),
    ("resend, bytes after the payloads", group_resend(gid, [env1]) + b"\x00"),
    ("resend, seventeen payloads", group_resend(gid, [env2] * 17)),
    ("sender keys, none", bytes([SENDER_KEYS]) + gid + bytes([0])),
    ("sender keys, cut in an entry", sender_keys(gid, keys[0][1])[:-3]),
    ("sender keys, bytes after the entries", sender_keys(gid, keys[0][1]) + b"\x00"),
    ("group state, two bytes after the members", group_state(gid, 3, camille, "Rando Vercors", three) + b"\x01\x02"),
]
for nm, d in bad:
    print("[[bad_group_control]]")
    print(f'name = "{nm}"')
    print(f'datagram = "{d.hex()}"')
    print()


def small_file(ts: int, name: str, data: bytes) -> bytes:
    n = name.encode()
    return bytes([FILE]) + struct.pack("<I", ts) + bytes([2, len(n)]) + n + data


def deflate(data: bytes) -> bytes:
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def compressed_file(ts: int, name: str, size: int, stream: bytes) -> bytes:
    n = name.encode()
    return bytes([FILE]) + struct.pack("<I", ts) + bytes([3, len(n)]) + n + struct.pack("<I", size) + stream


def noise(n: int) -> bytes:
    """Incompressible bytes."""
    return b"".join(hashlib.sha256(struct.pack("<I", i)).digest() for i in range(n // 32 + 1))[:n]


def track(points: int) -> bytes:
    """A GPX track shaped like a hiking route export (§13.2 kind 3, ADR-0023)."""
    out = ['<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="lichen vectors">\n<trk><name>Corniche</name><trkseg>\n']
    lat, lon, ele = 45_070_000, 5_520_000, 1_200
    for i in range(points):
        lat += (i * 37) % 23 - 9
        lon += (i * 53) % 29 - 11
        ele += (i * 11) % 7 - 3
        out.append(f'<trkpt lat="{lat / 1e6:.6f}" lon="{lon / 1e6:.6f}"><ele>{ele}</ele></trkpt>\n')
    out.append("</trkseg></trk>\n</gpx>\n")
    return "".join(out).encode()


# Uncompressed (kind 2): too short to gain, or incompressible.
files = [
    ("a note", 1_793_001_000, "note.txt", b"Refuge de la Pra, code 4521.\n"),
    ("a photo, accents in the name", 1_793_001_001, "Été_Vercors.jpg", noise(1024)),
    ("the largest", 1_793_001_002, "a" * 64, noise(8192)),
]
for nm, ts, fname, data in files:
    print("[[file]]")
    print(f'name = "{nm}"')
    print(f"ts = {ts}")
    print(f'file_name = "{fname}"')
    print(f'data = "{data.hex()}"')
    print(f'datagram = "{small_file(ts, fname, data).hex()}"')
    print()

# Compressed (kind 3): decoding is fixed, the encoder's bytes are not (ADR-0023).
gpx = track(300)
compressed = [
    ("a compressed track", 1_793_001_003, "corniche.gpx", gpx),
    ("a compressed note", 1_793_001_004, "note.txt", b"Refuge de la Pra, code 4521. " * 40),
]
for nm, ts, fname, data in compressed:
    stream = deflate(data)
    assert len(stream) <= 8192
    print("[[file]]")
    print(f'name = "{nm}"')
    print(f"ts = {ts}")
    print(f'file_name = "{fname}"')
    print(f'data = "{data.hex()}"')
    print(f'datagram = "{compressed_file(ts, fname, len(data), stream).hex()}"')
    print("compressed = true")
    print()

bad_files = [
    ("no name", bytes([FILE]) + struct.pack("<I", 1) + bytes([2, 0]) + b"x"),
    ("no content", small_file(1, "a.txt", b"")),
    ("over 8 KB", small_file(1, "a.bin", bytes(8193))),
    ("a name over 64 bytes", small_file(1, "b" * 65, b"x")),
    ("a name that is not UTF-8", bytes([FILE]) + struct.pack("<I", 1) + bytes([2, 2]) + b"\xff\xfe" + b"x"),
    ("cut in the name", bytes([FILE]) + struct.pack("<I", 1) + bytes([2, 9]) + b"a.t"),
    ("compressed: inflates to more than its size", compressed_file(1, "a.txt", 10, deflate(b"a" * 11))),
    ("compressed: inflates to less than its size", compressed_file(1, "a.txt", 12, deflate(b"a" * 11))),
    ("compressed: size 0", compressed_file(1, "a.txt", 0, deflate(b"a"))),
    ("compressed: size over 64 KiB", compressed_file(1, "a.bin", 65_537, deflate(bytes(65_537)))),
    ("compressed: a bomb, 1 MiB of zeros", compressed_file(1, "a.bin", 65_536, deflate(bytes(1 << 20)))),
    ("compressed: stream cut short", compressed_file(1, "a.txt", 600, deflate(b"Refuge " * 100)[:-3])),
    ("compressed: no stream", compressed_file(1, "a.txt", 1, b"")),
    ("compressed: over 8 KB on the air", compressed_file(1, "a.bin", 9000, deflate(noise(9000)))),
    ("compressed: not DEFLATE", compressed_file(1, "a.txt", 4, b"\xff\xff\xff\xff")),
    ("compressed: size cut", bytes([FILE]) + struct.pack("<I", 1) + bytes([3, 5]) + b"a.txt" + b"\x04\x00"),
]
for nm, d in bad_files:
    print("[[bad_file]]")
    print(f'name = "{nm}"')
    print(f'datagram = "{d.hex()}"')
    print()


def sticker(ts: int, s: int, emoji: str) -> bytes:
    return bytes([STICKER]) + struct.pack("<I", ts) + bytes([s]) + emoji.encode()


def gif(ts: int, provider: int, w: int, h: int, gid: str, title: str) -> bytes:
    i = gid.encode()
    return bytes([GIF]) + struct.pack("<IBHHB", ts, provider, w, h, len(i)) + i + title.encode()


stickers = [
    ("tears of joy", 1_793_002_000, 1, "\U0001F602"),
    ("red heart, with its variation selector", 1_793_002_001, 1, "\u2764\ufe0f"),
    ("an unknown set", 1_793_002_002, 9, "\U0001F44D"),
]
for nm, ts, st, emoji in stickers:
    print("[[sticker]]")
    print(f'name = "{nm}"')
    print(f"ts = {ts}")
    print(f"set = {st}")
    print(f'emoji = "{emoji}"')
    print(f'datagram = "{sticker(ts, st, emoji).hex()}"')
    print()

gifs = [
    ("a GIPHY GIF with its title", 1_793_002_010, 1, 200, 150, "3o7abKhOpu0NwenH3O", "happy dance"),
    ("no title, shape unknown", 1_793_002_011, 1, 0, 0, "l0MYt5jPR6QX5pnqM", ""),
    ("an unknown provider", 1_793_002_012, 7, 200, 200, "abc123", "a cat"),
]
for nm, ts, prov, w, h, gid_, title in gifs:
    print("[[gif]]")
    print(f'name = "{nm}"')
    print(f"ts = {ts}")
    print(f"provider = {prov}")
    print(f"width = {w}")
    print(f"height = {h}")
    print(f'id = "{gid_}"')
    print(f'title = "{title}"')
    print(f'datagram = "{gif(ts, prov, w, h, gid_, title).hex()}"')
    print()

bad_content = [
    ("sticker, no emoji", sticker(1, 1, "")),
    ("sticker, 33 bytes", sticker(1, 1, "a" * 33)),
    ("sticker, not UTF-8", bytes([STICKER]) + struct.pack("<I", 1) + bytes([1, 0xFF])),
    ("gif, empty id", gif(1, 1, 0, 0, "", "x")),
    ("gif, an id with a slash", gif(1, 1, 0, 0, "ab/cd", "")),
    ("gif, an id with a dot", gif(1, 1, 0, 0, "..", "")),
    ("gif, an id of 33 bytes", gif(1, 1, 0, 0, "a" * 33, "")),
    ("gif, a title of 65 bytes", gif(1, 1, 0, 0, "abc", "t" * 65)),
    ("gif, id length past the end", bytes([GIF]) + struct.pack("<IBHHB", 1, 1, 0, 0, 20) + b"abc"),
    ("gif, cut in the header", bytes([GIF, 1, 0, 0])),
]
for nm, d in bad_content:
    print("[[bad_content]]")
    print(f'name = "{nm}"')
    print(f'datagram = "{d.hex()}"')
    print()

links = [
    ("a GIF page", "https://giphy.com/gifs/happy-dance-3o7abKhOpu0NwenH3O", "3o7abKhOpu0NwenH3O", "happy dance"),
    ("www, a query", "https://www.giphy.com/gifs/cat-funny-l0MYt5jPR6QX5pnqM?utm_source=x", "l0MYt5jPR6QX5pnqM", "cat funny"),
    ("a page without a slug", "https://giphy.com/gifs/l0MYt5jPR6QX5pnqM", "l0MYt5jPR6QX5pnqM", ""),
    ("the media host", "https://media.giphy.com/media/3o7abKhOpu0NwenH3O/giphy.gif", "3o7abKhOpu0NwenH3O", ""),
    ("a numbered media host, v1 token", "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/3o7abKhOpu0NwenH3O/200w.webp", "3o7abKhOpu0NwenH3O", ""),
    ("i.giphy.com", "https://i.giphy.com/3o7abKhOpu0NwenH3O.webp", "3o7abKhOpu0NwenH3O", ""),
    ("spaces around", "  https://giphy.com/gifs/wave-abc123  ", "abc123", "wave"),
]
for nm, text, gid_, title in links:
    print("[[gif_link]]")
    print(f'name = "{nm}"')
    print(f'text = "{text}"')
    print(f'id = "{gid_}"')
    print(f'title = "{title}"')
    print()

not_links = [
    ("plain http", "http://giphy.com/gifs/happy-3o7abKhOpu0NwenH3O"),
    ("another host", "https://evil.example/gifs/happy-3o7abKhOpu0NwenH3O"),
    ("a look-alike host", "https://giphy.com.evil.example/gifs/x-abc"),
    ("text around the link", "look https://giphy.com/gifs/x-abc"),
    ("a stickers page", "https://giphy.com/stickers/x-abc"),
    ("an id with a dot", "https://i.giphy.com/ab.cd.webp.png"),
    ("a path climbing out", "https://media.giphy.com/media/../x/giphy.gif"),
]
for nm, text in not_links:
    print("[[not_gif_link]]")
    print(f'name = "{nm}"')
    print(f'text = "{text}"')
    print()


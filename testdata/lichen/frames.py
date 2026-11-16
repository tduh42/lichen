#!/usr/bin/env python3
"""lichen frame vectors: session frames (DATA, ACK, ABORT, protocol.md §3, §5, §7.2), group
envelopes (§10.3) and PROBEs (§4.5), sealed with ChaCha20-Poly1305 and its full 16-byte tag
(ADR-0028).

An independent oracle: ChaCha20, Poly1305 and the AEAD are written here from RFC 8439 and
checked against its test vectors, HKDF comes from Python's hmac. Nothing is shared with the Rust
code. Regenerate with:
    python3 -I testdata/lichen/frames.py > testdata/lichen/frames.toml
"""
import hashlib
import hmac
import struct

MASK = 0xFFFFFFFF


def rotl(v, n):
    return ((v << n) & MASK) | (v >> (32 - n))


def quarter(s, a, b, c, d):
    s[a] = (s[a] + s[b]) & MASK; s[d] = rotl(s[d] ^ s[a], 16)
    s[c] = (s[c] + s[d]) & MASK; s[b] = rotl(s[b] ^ s[c], 12)
    s[a] = (s[a] + s[b]) & MASK; s[d] = rotl(s[d] ^ s[a], 8)
    s[c] = (s[c] + s[d]) & MASK; s[b] = rotl(s[b] ^ s[c], 7)


def chacha20_block(key: bytes, counter: int, nonce: bytes) -> bytes:
    init = list(struct.unpack("<4I", b"expand 32-byte k")) + list(struct.unpack("<8I", key)) \
        + [counter] + list(struct.unpack("<3I", nonce))
    s = init[:]
    for _ in range(10):
        quarter(s, 0, 4, 8, 12); quarter(s, 1, 5, 9, 13); quarter(s, 2, 6, 10, 14); quarter(s, 3, 7, 11, 15)
        quarter(s, 0, 5, 10, 15); quarter(s, 1, 6, 11, 12); quarter(s, 2, 7, 8, 13); quarter(s, 3, 4, 9, 14)
    return struct.pack("<16I", *[(a + b) & MASK for a, b in zip(s, init)])


def chacha20(key: bytes, counter: int, nonce: bytes, data: bytes) -> bytes:
    out = bytearray()
    for i in range(0, len(data), 64):
        block = chacha20_block(key, counter + i // 64, nonce)
        out += bytes(a ^ b for a, b in zip(data[i:i + 64], block))
    return bytes(out)


def poly1305(key: bytes, msg: bytes) -> bytes:
    r = int.from_bytes(key[:16], "little") & 0x0FFFFFFC0FFFFFFC0FFFFFFC0FFFFFFF
    s = int.from_bytes(key[16:], "little")
    p, acc = (1 << 130) - 5, 0
    for i in range(0, len(msg), 16):
        block = msg[i:i + 16]
        acc = (acc + int.from_bytes(block, "little") + (1 << (8 * len(block)))) * r % p
    return ((acc + s) % (1 << 128)).to_bytes(16, "little")


def pad16(b: bytes) -> bytes:
    return b + bytes(-len(b) % 16)


def seal(key: bytes, nonce: bytes, aad: bytes, plain: bytes):
    """RFC 8439 §2.8: (ciphertext, 16-byte tag)."""
    ct = chacha20(key, 1, nonce, plain)
    otk = chacha20_block(key, 0, nonce)[:32]
    tag = poly1305(otk, pad16(aad) + pad16(ct) + struct.pack("<QQ", len(aad), len(ct)))
    return ct, tag


def self_check():
    # RFC 8439 §2.4.2 (ChaCha20), §2.5.2 (Poly1305), §2.8.2 (AEAD).
    key = bytes(range(32))
    text = (b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the "
            b"future, sunscreen would be it.")
    ct = chacha20(key, 1, bytes.fromhex("000000000000004a00000000"), text)
    assert ct[:16].hex() == "6e2e359a2568f98041ba0728dd0d6981"
    mac = poly1305(bytes.fromhex("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b"),
                   b"Cryptographic Forum Research Group")
    assert mac.hex() == "a8061dc1305136c6c22b8baf0c0127a9"
    ct, tag = seal(bytes(range(0x80, 0xA0)), bytes.fromhex("070000004041424344454647"),
                   bytes.fromhex("50515253c0c1c2c3c4c5c6c7"), text)
    assert ct[:16].hex() == "d31a8d34648e60db7b86afbc53ef7ec2"
    assert tag.hex() == "1ae10b594f09e26a7e902ecbd0600691"


def hkdf(salt: bytes, ikm: bytes, info: bytes, n: int = 32) -> bytes:
    prk = hmac.new(salt, ikm, hashlib.sha256).digest()
    out, t, i = b"", b"", 1
    while len(out) < n:
        t = hmac.new(prk, t + info + bytes([i]), hashlib.sha256).digest()
        out, i = out + t, i + 1
    return out[:n]


DATA, ACK, ABORT = 0, 1, 2


def header(dst, src, tag, kind, index, total, attempt) -> bytes:
    return bytes([dst, src]) + struct.pack("<H", tag) + bytes([kind << 6 | index, total, attempt])


def session_frame(key, hdr: bytes, plain: bytes) -> bytes:
    """§3.1, §5: the nonce is header[2..7] and seven zero bytes; the AAD is the header."""
    ct, tag = seal(key, hdr[2:7] + bytes(7), hdr, plain)
    return hdr + ct + tag


def ack_body(total: int, bitmap: int, complete: bool) -> bytes:
    """§7.2: bit 0 COMPLETE, bit i + 1 for fragment i, in ⌈(total + 1) / 8⌉ bytes."""
    return ((bitmap << 1) | int(complete)).to_bytes((total + 1 + 7) // 8, "little")


def envelope(key, gid_hash, sender, key_epoch, counter, index, total, copy, plain) -> bytes:
    """§10.3: padded with one zero byte (type 0x03) when a stock node would otherwise try it."""
    frag = index << 4 | copy << 3 | total
    padded = (11 + len(plain) + 16 - 3) % 16 == 0
    hdr = bytes([0x7C, 0x03 if padded else 0x02]) + struct.pack("<H", gid_hash) \
        + bytes([sender, key_epoch]) + struct.pack("<I", counter) + bytes([frag])
    ct, tag = seal(key, struct.pack("<I", counter) + bytes([key_epoch, frag]) + bytes(6), hdr, plain)
    return hdr + ct + tag + (b"\x00" if padded else b"")


def probe(s, prober, target, dst, src, pid, anonymous) -> bytes:
    """§4.5: version 2, a 16-byte MAC over the first 12 bytes, nonce 0, no plaintext."""
    label = b"lichen/1 probe-anon" if anonymous else b"lichen/1 probe"
    key = hkdf(pid, bytes(32) if anonymous else s, label + prober + target)
    aad = bytes([0x7C, 0x02, dst, src]) + pid
    _, mac = seal(key, bytes(12), aad, b"")
    return aad + mac


def table(kind: str, **fields):
    print(f"[[{kind}]]")
    for k, v in fields.items():
        if isinstance(v, bool):
            print(f"{k} = {'true' if v else 'false'}")
        elif isinstance(v, int):
            print(f"{k} = {v}")
        else:
            print(f'{k} = "{v.hex()}"')
    print()


def main():
    self_check()
    print("# lichen frame vectors (ADR-0028: RFC 8439, 16-byte tags). Generated by frames.py: do not edit.\n")
    k1, k2 = bytes([1]) * 32, bytes([2]) * 32  # A's send key, B's send key
    a, b = 0xA1, 0xB2

    # A short datagram, then the 8 fragments of 146 bytes of a long one (ADR-0031).
    hello = b"\x01" + struct.pack("<I", 1_793_000_000) + "Salut Léa, on se retrouve au col ?".encode()
    table("data", key=k1, dst=b, src=a, tag=0, index=0, total=1, attempt=0, plain=hello,
          frame=session_frame(k1, header(b, a, 0, DATA, 0, 1, 0), hello))
    long = bytes((7 * i + 3) % 256 for i in range(8 * 146))
    for i in range(8):
        part = long[146 * i:146 * (i + 1)]
        table("data", key=k1, dst=b, src=a, tag=1, index=i, total=8, attempt=0, plain=part,
              frame=session_frame(k1, header(b, a, 1, DATA, i, 8, 0), part))

    # ACKs from B: complete for one fragment (one packed byte), partial then complete for
    # eight (two bytes), complete for 64 (nine bytes).
    for tag, total, seq, bitmap, complete in ((0, 1, 0, 1, True), (1, 8, 0, 0x80, False),
                                              (1, 8, 1, 0xFF, True), (2, 64, 0, (1 << 64) - 1, True)):
        body = ack_body(total, bitmap, complete)
        table("ack", key=k2, dst=a, src=b, tag=tag, total=total, seq=seq, body=body,
              complete=complete, frame=session_frame(k2, header(a, b, tag, ACK, 0, total, seq), body))

    # A gives up its datagram: ABORT index 0, reason 4 (cancelled).
    table("abort", key=k1, dst=b, src=a, tag=0, index=0, total=1, reason=4,
          frame=session_frame(k1, header(b, a, 0, ABORT, 0, 1, 0), b"\x04"))

    # Group envelopes under a sender key: 24 bytes of plaintext need the pad byte (the payload
    # would be 3 + 48 bytes), 20 do not, 128 is the largest fragment (ADR-0031).
    sk = bytes(range(0x40, 0x60))
    for n in (20, 24, 128):
        plain = bytes((5 * i + n) % 256 for i in range(n))
        payload = envelope(sk, 0xBEEF, 0xC3, 2, 41, 0, 1, 0, plain)
        table("envelope", key=sk, gid_hash=0xBEEF, sender=0xC3, key_epoch=2, counter=41, index=0,
              total=1, copy=0, plain=plain, padded=payload[1] == 0x03, payload=payload)
    # A group voice note's fragments (ADR-0035): `total` 5 sets bit 2 of `frag`; the first
    # (full) fragment of the first copy, the last (86 bytes) of the second.
    for index, copy, n in ((0, 0, 128), (4, 1, 86)):
        plain = bytes((7 * i + index) % 256 for i in range(n))
        payload = envelope(sk, 0xBEEF, 0xC3, 2, 42, index, 5, copy, plain)
        table("envelope", key=sk, gid_hash=0xBEEF, sender=0xC3, key_epoch=2, counter=42, index=index,
              total=5, copy=copy, plain=plain, padded=payload[1] == 0x03, payload=payload)


    # PROBEs: keyed by the two identities, and anonymous (keyed by the node keys).
    s = bytes(range(0x10, 0x30))
    prober, target = bytes([0xA1]) + bytes(31), bytes([0xB2]) + bytes(31)
    pid = bytes(range(8))
    for anonymous in (False, True):
        table("probe", s=s, prober=prober, target=target, dst=0xB2, src=0xA1, id=pid,
              anonymous=anonymous, payload=probe(s, prober, target, 0xB2, 0xA1, pid, anonymous))


if __name__ == "__main__":
    main()

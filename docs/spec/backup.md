# lichen backup file (normative, ADR-0037)

One file that restores a phone's lichen data on a new phone, or on the same one after a
reinstall: its messages, contacts, groups, conversation preferences, settings and media. It is
encrypted under a key derived from the identity seed, so the 24 words of the recovery phrase
(protocol.md §2) open it, and nothing else does.

Integers are **little-endian**. `‖` = concatenation. HKDF is HKDF-SHA256 (RFC 5869) and the AEAD
ChaCha20-Poly1305 as RFC 8439 defines it, with its full 16-byte tag (as protocol.md §5).

## 1. Keys

```
salt  = 32 random bytes, new for every backup
key   = HKDF(salt, seed, "lichen/1 backup key")            (32 bytes)
check = HKDF(salt, seed, "lichen/1 backup check")[0..16]
```

`seed` is the identity's 32-byte seed, the entropy of the 24 words. The identity key itself is
never used, and the seed only feeds HKDF. A fresh salt gives every backup its own key, so the
chunk nonces below, which only count, never repeat under a key. `check` lets a reader tell words
that are not this backup's from a damaged file; it reveals nothing of the seed.

## 2. File

```
file   = header ‖ chunk_0 ‖ chunk_1 ‖ … ‖ chunk_n
header = "lichen/backup" (13 bytes, ASCII) ‖ version (1) = 0x01 ‖ salt (32) ‖ check (16)     (62 bytes)
chunk_i = ChaCha20-Poly1305(key, nonce_i, AAD = header, plain_i) ‖ tag (16)
nonce_i = i u64 ‖ last (1) ‖ 0x00 0x00 0x00
```

- `plain_i` holds 65,536 bytes in every chunk but the last, which holds 0 to 65,536 bytes and
  has `last = 1` (`0` in the others). There is always at least one chunk. A reader knows the
  last chunk by the end of the file: every other chunk is exactly 65,552 bytes long.
- The chunks, in order, carry the **contents** (§3). Nothing of a chunk is used before its tag
  verifies.
- A file cut short ends on a chunk sealed with `last = 0` (or in the middle of one), chunks
  swapped or moved fail their nonce, bytes appended make the true last chunk a middle one: each
  fails a tag, and the file is refused as **damaged**.
- Another `version`, or a file that does not start with the magic, is refused as **not a lichen
  backup** (or one made by a newer lichen). A `check` that differs means **other words**.

## 3. Contents

The plaintext of the chunks, concatenated, is a list of entries:

```
entry = kind (1) ‖ name_len (1) ‖ name (name_len bytes of UTF-8) ‖ size u64 ‖ data (size bytes)
```

| kind | name | data |
|---|---|---|
| 0x00 END | empty | empty: the last entry; nothing follows it |
| 0x01 DATABASE | empty | the phone's store (ADR-0010), a SQLite 3 database, exactly once |
| 0x02 FILE | the file's name in the app's `downloads` folder | the file: a medium fetched over the Internet bridge (§14), or one we offered; not one still downloading (`….part`) |
| 0x03 SETTINGS | empty | the app's settings (§4), at most once, at most 65,536 bytes |

A FILE name is 1 to 128 bytes of `A`–`Z`, `a`–`z`, `0`–`9`, `.`, `_`, `-`, not starting with `.`:
never a path. Another kind, a second DATABASE or SETTINGS, a contents that ends before END or goes
on after it, refuse the file as damaged.

**The database** is the store at the moment of the export, less what a restore must not bring
back (ADR-0037):

- the other members' **sender keys** and replay windows: each group keeps its state, its
  members and our key epoch (protocol.md §10.2; our own key is never stored);
- group **invitations** not answered yet (they hold sender keys);
- the **outbox**: texts still queued or sending are marked failed (interrupted), and our
  reactions, edits and deletes not delivered yet are left out (§6.1);
- **disappearing messages** (a timer was on when they were stored, §6.5), with their media,
  reactions and edits: a file outlives their time;
- **location**: the shares we were giving and the positions received (§12);
- search **consents** (§11) and the radio's state (the TX power a search lowered);
- the nodes' **signal readings** and the **readings they sent** (§16.1, §16.7): they describe one radio's surroundings at one time.

Sessions are never stored (ADR-0003), so none is in a backup either.

## 4. Settings

A JSON object (UTF-8). Its keys are the names both apps give their settings, its values JSON
strings, booleans or integers:

| key | value |
|---|---|
| `lichen.myName` | string: the name we show |
| `lichen.tor`, `lichen.iroh`, `lichen.relayOnly` | booleans: Internet bridge transports (ADR-0015) |
| `lichen.gifAuto` | boolean: GIFs load by themselves (ADR-0025) |
| `lichen.voiceMode` | integer: the Codec2 mode of new voice messages, 0, 2 or 3 (§13.1) |
| `lichen.radioDetails` | boolean: fragments, airtime and signal under every message, and the radio card on Messages |
| `appearance` | string: `system`, `light` or `dark` |
| `map.style` | string: `auto`, `dark`, `liberty`, `positron` or `bright` |

A restoring app takes the keys it knows, with values of the right type and range (strings
compared without case), and ignores the others.
The app lock, the paired radio and anything tied to the old phone are not settings of the backup.

## 5. Restoring

1. Read the header: the magic, the version, then `check` from the typed words. Nothing is written
   before these three pass.
2. Decrypt the chunks in order into a folder of the app's own, the entries as files.
3. Only once the last chunk and END have verified, install: the database replaces the store of
   the identity the words give (its file renamed into place, any `-wal`, `-shm` or `-journal`
   left by the old one removed first), and FILE entries are added to `downloads`. A failure
   before this point leaves the phone as it was, and the folder is removed.
4. The app keeps the seed of the words as its identity, applies the settings, and starts as after
   a restart: our new group keys go out (§10.2) and every contact gets a new session at the next
   message (§4).

Vectors: `testdata/lichen/backup.toml` (`backup.py`, an independent oracle).

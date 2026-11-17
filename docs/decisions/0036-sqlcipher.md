# ADR-0036 — The database encrypted at rest (SQLCipher)

- Status: accepted (2026-11-17: OpenSSL built from source approved for Android)
- Date: 2026-11-16
- Revisits ADR-0010 ("encrypting the database with a Keychain key: to revisit")

## Context

The store (ADR-0010) holds the history, the contacts' identities and the other members' sender
keys in clear inside the app's sandbox, protected by the OS only (file protection after the first
unlock on iOS, the app's private files on Android). A phone read below the app sandbox (a
forensic tool on an unlocked or after-first-unlock device, a rooted phone, a backup of the raw
storage) reads them. SQLCipher is the usual answer and is approved C code; it needs a crypto
library under it, and that is where the platforms differ.

What `rusqlite` 0.40 / `libsqlite3-sys` 0.38.2 do (bundled SQLCipher 4.14.0 on SQLite 3.51.3,
read in its `build.rs`):

| feature | Apple targets (iOS, simulator, macOS host) | Android | Linux host (CI) |
|---|---|---|---|
| `bundled-sqlcipher` | CommonCrypto (`SQLCIPHER_CRYPTO_CC`), links Security and CoreFoundation: **no extra C** (built and linked on this branch) | links `-lcrypto`, which the NDK's API 31 sysroot does not have: **does not build** | the system's `libcrypto` (OpenSSL 3, dynamic; tests only) |
| `bundled-sqlcipher-vendored-openssl` | OpenSSL built from source instead of CommonCrypto | OpenSSL built from source (`openssl-src`), linked statically: **builds** | same |

Features can differ per target (`[target.'cfg(…)'.dependencies]`, resolver 3), so Apple can keep
CommonCrypto whatever Android uses.

## Options for Android

1. **Vendored OpenSSL** (`openssl-src` 300.6.1, OpenSSL 3.6.3, Apache-2.0): 548,000 lines of C
   (344,000 in `crypto/`), built for `aarch64-linux-android` by its own Perl configure; only
   `libcrypto` is linked (AES-256-CBC, HMAC-SHA512, PBKDF2, random). It builds with the NDK as it
   is, and adds 4 MB to the APK (below). Maintained upstream; advisories in parts lichen does not
   reach (TLS is in `libssl`, not linked) still mean dependency updates.
2. **A crypto provider in Rust**: SQLCipher calls a provider through a table of functions
   (`SQLCIPHER_CRYPTO_CUSTOM`); one written over RustCrypto (`aes`, `cbc`, `hmac`, `sha2`,
   `pbkdf2`) adds no C, but adds `unsafe` FFI code (a second module beside `codec2_c.rs`) and needs
   a patched `libsqlite3-sys` (its `build.rs` links `-lcrypto` on every non-Apple target). Our
   own crypto glue under the database: to be fuzzed and checked against SQLCipher's vectors.
3. **libtomcrypt** (public domain, a much smaller C library): a provider SQLCipher supports; the
   same patched `build.rs`, and one more C library of ours to build for Android.
4. **SQLite3 Multiple Ciphers** in place of SQLCipher: its own ciphers in C, no crypto library,
   reads SQLCipher's format; not approved, and not supported by `rusqlite`.

Chosen: **option 1** for Android (and Linux hosts), CommonCrypto on Apple. It is the path
`rusqlite` supports (no fork, no `unsafe` of ours), the C is a mainstream library built
reproducibly from crates.io, and its cost is size and build time, measured below. Option 2
stays the fallback should OpenSSL become a burden.

## Decision

- The store is a SQLCipher database (SQLCipher 4 defaults: AES-256-CBC, HMAC-SHA512 per page),
  opened with a **raw 32-byte key** (`PRAGMA key = "x'…'"`): no passphrase to stretch.
- **The key**: 32 random bytes drawn at the first start with SQLCipher, then always read. iOS: a
  Keychain item, this device only, after the first unlock (as the identity, ADR-0013). Android: a
  file in the app's no-backup files, sealed with AES-GCM under an Android Keystore key that never
  leaves the Keystore. A key that cannot be read now (before the first unlock) opens no database:
  the app says so ("try again"), never opens a clear one. A database already encrypted is never
  given a new key: when its key is gone for good (a reset Keychain or Keystore), the app says the
  key is lost and offers to restore a backup (ADR-0037) or to start again with an empty history
  (the unreadable database is removed, a new key drawn; the identity stays).
- **Migration**: a plain database found at start is encrypted in place: `sqlcipher_export` into a
  copy beside it (its `user_version` carried over), synced, renamed over the plain file (atomic),
  its journal files removed. Interrupted, the plain file is untouched and the next start does it
  again. The plain file's blocks are freed, not overwritten: flash storage promises no more.
- **Backups** (ADR-0037) take a plain copy (`sqlcipher_export` into `KEY ''`) on a connection of
  their own and encrypt it themselves; a restored database is plain until the next start
  encrypts it with this phone's key.
- **All platforms at once**: one switch, the `sqlcipher` feature of `lc-core` and `lc-ffi`, on in
  the apps' core builds (`scripts/build-ios-core.sh`, `scripts/build-android-core.sh`): Apple
  targets build SQLCipher on CommonCrypto, every other target on OpenSSL's libcrypto built from
  source (`libsqlite3-sys` with `bundled-sqlcipher-vendored-openssl`, a target-specific
  dependency of `lc-core`). The apps ask `database_encryption()` and only then keep a key. A core
  without SQLCipher refuses a key (nothing stored) rather than ignoring it.
- **Tests**: `cargo test --workspace` runs on plain SQLite; `scripts/check.sh` also runs the store,
  the backups and the binding layer under SQLCipher (`tests/sqlcipher.rs`, `tests/backup.rs`,
  `lc-ffi` unit tests), on macOS with CommonCrypto and on Linux (CI) with OpenSSL from source.

## Consequences

- Measured on 2026-11-17 (release core, arm64; `docs/measurements.md`):

  | | SQLite | SQLCipher | difference |
  |---|---|---|---|
  | Android `liblc_ffi.so`, stripped (OpenSSL from source) | 33,225,008 B | 37,224,272 B | +4.0 MB (+12 %) |
  | Android release APK (native libraries stored uncompressed) | 54,152,810 B | 58,166,890 B | +4.0 MB (+7.4 %) |
  | iOS app (CommonCrypto, measured on 2026-11-16) | | | +49 KB |

  On iOS the app links as it is: CommonCrypto is part of the system library, and the app
  already links `Security` (its Keychain code imports it).
- **Build time**: OpenSSL compiled from source takes 30 s (100 s of CPU) per target on a 12-core
  Mac, once: Cargo and CI's cache keep it. CI builds it for Android and for the Linux host (the
  JVM tests' core and `check.sh`'s SQLCipher step); expect about a minute each on a 4-core runner
  the first time. `check.sh` also compiles the binding layer once more with SQLCipher (about a
  minute on the Mac).
- **First start after the update**: a history of 100,000 messages and 300 images (34 MB) is
  encrypted in place in 0.15 s on the Mac (release); a phone is slower, but it happens once and
  grows with the database. Opening an encrypted database takes 1 ms.
- Every read and write pays AES and an HMAC per 4 KiB page; lichen's database is small and its
  writes are few (a message, a status).
- Rejected: encrypting columns in the app (search, indexes and every query would change; the
  schema would still leak), Android's own SQLCipher AAR (the store is in Rust, through rusqlite),
  a key derived from the identity seed (one secret for two purposes; a restored identity would
  open an old phone's databases).

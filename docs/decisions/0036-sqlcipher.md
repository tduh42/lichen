# ADR-0036 — The database encrypted at rest (SQLCipher)

- Status: proposed: waiting for the choice of a crypto library on Android (below)
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
| `bundled-sqlcipher` | CommonCrypto (`SQLCIPHER_CRYPTO_CC`), links Security and CoreFoundation: **no extra C** | links `-lcrypto`, which the NDK does not have: **does not build** | the system's `libcrypto` (OpenSSL 3, dynamic; tests only) |
| `bundled-sqlcipher-vendored-openssl` | OpenSSL built from source instead of CommonCrypto | OpenSSL built from source (`openssl-src`), linked statically: **builds** | same |

Features can differ per target (`[target.'cfg(…)'.dependencies]`, resolver 3), so Apple can keep
CommonCrypto whatever Android uses.

## Options for Android

1. **Vendored OpenSSL** (`openssl-src`, OpenSSL 3, Apache-2.0): about 500,000 lines of C, built for
   `aarch64-linux-android` by its own Perl configure; only `libcrypto` is linked (AES-256-CBC,
   HMAC-SHA512, PBKDF2, random). Measured below. Maintained upstream; CVEs in parts lichen does
   not reach (TLS is in `libssl`, not linked) still mean dependency updates.
2. **A crypto provider in Rust**: SQLCipher calls a provider through a table of functions
   (`SQLCIPHER_CRYPTO_CUSTOM`); one written over RustCrypto (`aes`, `cbc`, `hmac`, `sha2`,
   `pbkdf2`) adds no C, but adds `unsafe` FFI code (a second module beside `codec2_c.rs`) and needs
   a patched `libsqlite3-sys` (its `build.rs` links `-lcrypto` on every non-Apple target). Our
   own crypto glue under the database: to be fuzzed and checked against SQLCipher's vectors.
3. **libtomcrypt** (public domain, about 60,000 lines of C): a provider SQLCipher supports; same
   patched `build.rs`, and one more C library to build for Android.
4. **SQLite3 Multiple Ciphers** in place of SQLCipher: its own ciphers in C, no crypto library,
   reads SQLCipher's format; not approved, and not supported by `rusqlite`.

Recommended: **option 1** for Android, CommonCrypto on Apple. It is the path `rusqlite` supports
(no fork, no `unsafe` of ours), the C is a mainstream library built reproducibly from crates.io,
and its cost is size, measured below. Option 2 is the fallback if the size or the C is refused.

## Decision (once the library is approved)

- The store is a SQLCipher database (SQLCipher 4 defaults: AES-256-CBC, HMAC-SHA512 per page),
  opened with a **raw 32-byte key** (`PRAGMA key = "x'…'"`): no passphrase to stretch.
- **The key**: 32 random bytes drawn once per phone. iOS: a Keychain item, this device only,
  after the first unlock (as the identity, ADR-0013). Android: a file in the app's no-backup
  files, sealed with AES-GCM under an Android Keystore key that never leaves the Keystore. A key
  that cannot be read now (before the first unlock) opens no database: the app shows its storage
  problem screen, never a clear database. A key that is gone for good (a reset Keystore) leaves
  the databases unreadable: the history comes back from a backup (ADR-0037).
- **Migration**: a plain database found at start is encrypted in place: `sqlcipher_export` into a
  copy beside it (its `user_version` carried over), synced, renamed over the plain file (atomic),
  its journal files removed. Interrupted, the plain file is untouched and the next start does it
  again. The plain file's blocks are freed, not overwritten: flash storage promises no more.
- **Backups** (ADR-0037) take a plain copy (`sqlcipher_export` into `KEY ''`) on a connection of
  their own and encrypt it themselves; a restored database is plain until the next start
  encrypts it with this phone's key.
- **All platforms at once**: one switch, `lc-ffi`'s `sqlcipher` feature; the apps ask
  `database_encryption()` and only then keep a key and pass it. A core without SQLCipher refuses
  a key (nothing stored) rather than ignoring it.

## What is ready (this branch), behind the switch, off

- `lc-core`: feature `sqlcipher`; `Store::open_encrypted`, `cipher::{encrypt_plain,
  export_plain}`; `tests/sqlcipher.rs` (a plain store encrypted in place, another key refused,
  the clear text gone from the file, a backup of an encrypted store restored and encrypted again)
  runs on macOS with CommonCrypto: `cargo test -p lc-core --features sqlcipher --test sqlcipher`.
  Without the feature, the same file checks that a key is refused and nothing is written.
- `lc-ffi`: feature `sqlcipher`, `LichenCore::with_database_key`, `database_encryption()`.
- Apps: the key in the Keychain (iOS) and behind the Keystore (Android), made and passed only
  when `database_encryption()` is true.

## Consequences

- Sizes (arm64, release, measured on this branch; see the table in the final report and
  `docs/measurements.md` once decided).
- Every read and write pays AES and an HMAC per 4 KiB page; lichen's database is small and its
  writes are few (a message, a status).
- Rejected: encrypting columns in the app (search, indexes and every query would change; the
  schema would still leak), Android's own SQLCipher AAR (the store is in Rust, through rusqlite),
  a key derived from the identity seed (one secret for two purposes; a restored identity would
  open an old phone's databases).

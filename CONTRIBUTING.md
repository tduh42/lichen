# Contributing

lichen is a personal project, maintained in spare time. Issues and pull requests are welcome; a security problem goes through private reporting instead ([`SECURITY.md`](SECURITY.md)).

## How a change lands

- One branch and one pull request per topic. `main` only takes a pull request, once CI has passed.
- CI (`.github/workflows/check.yml`) runs what `scripts/check.sh` runs (one version, `rustfmt`, `clippy -D warnings`, the tests, the `no_std` build, `cargo deny`), then the Android core, lint, unit tests and APK. Run `scripts/check.sh` before pushing.
- Pull requests are squash-merged: the description becomes the commit message, so it says what changed and why, and how it was tested.
- Every merge to `main` that passes CI is a release: CI tags it `vMAJOR.MINOR.N` and publishes the APK with the merged pull requests as notes.

## Versions

- [`VERSION`](VERSION) holds the series, `MAJOR.MINOR.0`; `Cargo.toml` and `apps/ios/project.yml` say the same (`scripts/check.sh` checks it).
- A release is `MAJOR.MINOR.N`, N the number of commits on `main` since `VERSION` last changed: with squash merges, one per pull request. `scripts/version.sh --version` prints it, `--build` the build number (every commit, always growing: Android `versionCode`, iOS `CFBundleVersion`).
- A milestone bumps the minor in `VERSION` (and the two files that follow it) in its own pull request: that merge is `MAJOR.MINOR.0`, the next ones `.1`, `.2`…
- Tags and releases are CI's: never move or edit one by hand.

## Rules the code follows

The full list is in [`CLAUDE.md`](CLAUDE.md), the working notes the maintainer and his coding assistant share. The ones that matter most:

- **Spec first.** A rule on the air starts in [`docs/spec/protocol.md`](docs/spec/protocol.md), then test vectors (`testdata/`), then code. A choice with alternatives gets a short decision record in [`docs/decisions/`](docs/decisions/).
- **Never panic on what comes from outside.** Every decoder of external input has a `cargo-fuzz` target (`fuzz/`); `scripts/fuzz.sh 60 <target>` runs one.
- **Nothing is interpreted before its tag is verified**, and a nonce never repeats under a key.
- **Stock MeshCore only:** nothing may assume a modified firmware.
- The repository is in English; the apps are in English and French.

## Building

See the [README](README.md): the Rust workspace, the iOS app (Xcode, XcodeGen, your own Apple team in `apps/ios/Local.xcconfig`) and the Android app (`scripts/build-android-core.sh`, Gradle).

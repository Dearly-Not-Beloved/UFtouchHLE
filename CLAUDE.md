# touchHLE UF: notes for coding agents

This is **touchHLE UF**, an unofficial fork of [touchHLE](https://github.com/touchHLE/touchHLE) (a high-level emulator for iPhone OS apps) with extra fixes for specific games, such as Inotia 1 and 2. Changes in this fork are AI-assisted. Upstream touchHLE does not accept AI-generated code (see `CONTRIBUTING.md`), so never submit anything from this fork upstream.

## Scope

- touchHLE emulates **32-bit ARM** (armv6/armv7) apps from the iPhone OS 2–4 era. Guest pointers are 32-bit throughout (`src/mem.rs`), and the CPU backend is dynarmic's A32 JIT. 64-bit-only (arm64) apps can't run.
- Game-specific workarounds should check that they are running the expected code (e.g. compare a function's start bytes) before patching or calling into the game.
- Don't commit app files (`touchHLE_apps/*.ipa`) or bundle them into builds.

## Building and testing

- macOS: set `CMAKE_POLICY_VERSION_MINIMUM=3.5`. Run `cargo build --release`, then the tests: `cargo test --release --lib` and `python3 dev-scripts/macos-integration-tests.py`.
- Before pushing: run `dev-scripts/lint.sh` and `dev-scripts/format.sh --check`. CI runs clang-format 23 on macOS and older versions on Linux and Windows, so formatting of C/Objective-C test code must satisfy both.
- Android: `dev-scripts/build-android-uf.sh` builds a signed APK (package `org.touchhleuf.android`). The person running it enters the keystore password; never store or pass secrets yourself.
- Test changes on macOS and then on a real Android phone before committing. The Android emulator's OpenGL ES 1.1 translation has bugs that real phones don't.

## Releases

Push a tag `vX.Y.Z`, then run the "Build touchHLE" workflow on it. Rename the artifacts to `touchHLE_UF_vX.Y.Z_{Android_AArch64.apk,macOS_x86_64.dmg,Windows_x86_64.zip}` and create a GitHub release with English notes.

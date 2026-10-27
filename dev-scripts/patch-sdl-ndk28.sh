#!/bin/bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# The Android build compiles SDL2 from the SDL source tree that cargo checks
# out for the rust-sdl2 dependency (sdl2-sys/build.rs builds "SDL" relative to
# CARGO_MANIFEST_DIR, i.e. <cargo checkout>/sdl2-sys/SDL).  That vendored SDL is
# SDL 2.26, which calls ALooper_pollAll().  NDK r28+ declares ALooper_pollAll
# with __REMOVED_IN(1,...), so calling it is a hard compile error ("unavailable:
# obsoleted").  ALooper_pollOnce has the same signature and is the replacement
# used by upstream SDL 2.28+.  This script replaces the single call site, and is
# idempotent so it can be run on every build (cargo re-checks out the dependency
# on a clean checkout, e.g. in CI).
#
# Usage: dev-scripts/patch-sdl-ndk28.sh
set -e

# Locate the SDL source tree that the rust-sdl2 (sdl2-sys) build actually uses.
SDL_FILE=""
while IFS= read -r f; do
    SDL_FILE="$f"
    break
done < <(find "$HOME/.cargo/git/checkouts" -type f \
    -path "*rust-sdl2-*/sdl2-sys/SDL/src/sensor/android/SDL_androidsensor.c" \
    2>/dev/null)

if [ -z "$SDL_FILE" ]; then
    echo "WARNING: could not locate sdl2-sys/SDL checkout; nothing to patch" >&2
    exit 0
fi

if grep -q "ALooper_pollOnce(0, NULL, &events" "$SDL_FILE"; then
    echo "SDL sensor already patched (ALooper_pollOnce): $SDL_FILE"
    exit 0
fi

if ! grep -q "ALooper_pollAll(0, NULL, &events" "$SDL_FILE"; then
    echo "WARNING: expected ALooper_pollAll not found in $SDL_FILE; skipping" >&2
    exit 0
fi

# Replace the single ALooper_pollAll call with ALooper_pollOnce (same signature).
perl -pi -e 's/ALooper_pollAll\(0, NULL, \&events, \(void\*\*\)\&source\)/ALooper_pollOnce(0, NULL, \&events, (void**)\&source)/' "$SDL_FILE"
echo "Patched SDL sensor ($SDL_FILE): ALooper_pollAll -> ALooper_pollOnce"

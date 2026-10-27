#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Upload a complete three-platform build to a draft; never publish it or
# replace assets belonging to an already published release.
set -euo pipefail

tag="${1:?Usage: upload-release-draft.sh TAG ASSET_DIRECTORY}"
asset_directory="${2:?Usage: upload-release-draft.sh TAG ASSET_DIRECTORY}"
: "${GH_REPO:?GH_REPO must name the destination repository}"

if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.-]+)?$ ]]; then
    echo "Not a supported version tag: $tag" >&2
    exit 1
fi

assets=(
    "$asset_directory/UFtouchHLE_${tag}_Android_AArch64.apk"
    "$asset_directory/UFtouchHLE_${tag}_macOS_x86_64.dmg"
    "$asset_directory/UFtouchHLE_${tag}_Windows_x86_64.zip"
)
for asset in "${assets[@]}"; do
    if [[ ! -s "$asset" ]]; then
        echo "Missing or empty release package: $asset" >&2
        exit 1
    fi
done

if draft="$(gh release view "$tag" --repo "$GH_REPO" --json isDraft --jq .isDraft)"; then
    if [[ "$draft" != true ]]; then
        echo "Release $tag is already published; refusing to change its assets." >&2
        exit 1
    fi
else
    gh release create "$tag" --repo "$GH_REPO" --verify-tag --draft \
        --title "UFtouchHLE $tag" \
        --notes "Build artifacts for $tag. Release notes are pending review."
fi

# Preserve existing draft notes and allow a failed upload to be retried.
if [[ "$(gh release view "$tag" --repo "$GH_REPO" --json isDraft --jq .isDraft)" != true ]]; then
    echo "Release $tag is no longer a draft; refusing to upload." >&2
    exit 1
fi
gh release upload "$tag" "${assets[@]}" --repo "$GH_REPO" --clobber

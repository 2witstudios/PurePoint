#!/bin/sh
# Runs meaningful Git/state tests in disposable repositories without the app build.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
output=$(mktemp -d /tmp/purepoint-git-review-check.XXXXXX)
trap 'rm -rf "$output"' EXIT
src="$repo/apps/purepoint-macos/purepoint-macos"
xcrun swiftc -D GIT_REVIEW_HARNESS -swift-version 5 -warnings-as-errors -default-isolation MainActor \
  -enable-upcoming-feature NonisolatedNonsendingByDefault -parse-as-library \
  "$src/Models/DiffModel.swift" "$src/Models/PRModel.swift" "$src/Models/WorkspaceModel.swift" "$src/Models/AgentStatus.swift" "$src/Models/ManifestModel.swift" \
  "$src/Services/GitService.swift" "$src/Services/WorktreeWatcher.swift" "$src/State/DiffState.swift" \
  "$repo/apps/purepoint-macos/purepoint-macosTests/GitReviewTests.swift" -o "$output/check"
"$output/check"

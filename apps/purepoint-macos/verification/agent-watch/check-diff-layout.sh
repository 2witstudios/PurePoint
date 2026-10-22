#!/bin/sh
# Exercise bounded AppKit diff layout without building/installing the application.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../../../.." && pwd)
output=$(mktemp -d /tmp/purepoint-diff-layout-check.XXXXXX)
trap 'rm -rf "$output"' EXIT
src="$repo/apps/purepoint-macos/purepoint-macos"
xcrun swiftc -D DIFF_LAYOUT_HARNESS -swift-version 5 -warnings-as-errors \
  -default-isolation MainActor -parse-as-library \
  "$src/Models/DiffModel.swift" "$src/Theme/Theme.swift" \
  "$src/Views/Detail/DiffContentNSView.swift" \
  "$repo/apps/purepoint-macos/purepoint-macosTests/DiffLayoutTests.swift" \
  -o "$output/check"
"$output/check"

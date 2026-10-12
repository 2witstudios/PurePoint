#!/bin/sh
# Standalone private-directory bootstrap proof; no app build phases or install.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
output=$(mktemp -d /tmp/purepoint-daemon-state-check.XXXXXX)
trap 'rm -rf "$output"' EXIT
xcrun swiftc -swift-version 5 -default-isolation MainActor -parse-as-library \
  "$repo/apps/purepoint-macos/purepoint-macos/Services/DaemonStateDirectory.swift" \
  "$repo/tools/verification/daemon-state-check.swift" -o "$output/check"
"$output/check"

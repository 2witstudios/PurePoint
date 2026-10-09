#!/bin/sh
# Standalone proof: compiles only channel state and its wire/client dependencies.
# Never invokes the app build phase or installs binaries.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
output=$(mktemp -d /tmp/purepoint-channel-check.XXXXXX)
trap 'rm -rf "$output"' EXIT
src="$repo/apps/purepoint-macos/purepoint-macos"
xcrun swiftc -swift-version 5 -default-isolation MainActor -parse-as-library \
  "$src/Models/ChannelModel.swift" "$src/Models/ManifestModel.swift" "$src/Models/AgentStatus.swift" \
  "$src/Services/DaemonProtocol.swift" "$src/Services/DaemonClient.swift" "$src/Services/DaemonConnection.swift" \
  "$src/State/ChannelState.swift" "$repo/tools/verification/channel-state-check.swift" -o "$output/check"
"$output/check" "$repo/docs/reference/fixtures/channel-history.json"

#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
output=$(mktemp -d /tmp/purepoint-daemon-lifecycle-check.XXXXXX)
trap 'rm -rf "$output"' EXIT
src="$repo/apps/purepoint-macos/purepoint-macos"
cp "$repo/tools/verification/daemon-lifecycle-fixture.py" "$output/pu-engine"
chmod 700 "$output/pu-engine"
xcrun swiftc -swift-version 5 -default-isolation MainActor -parse-as-library \
  "$src/Models/ChannelModel.swift" "$src/Models/ManifestModel.swift" "$src/Models/AgentStatus.swift" \
  "$src/Services/DaemonProtocol.swift" "$src/Services/DaemonClient.swift" "$src/Services/DaemonConnection.swift" \
  "$src/Services/DaemonStateDirectory.swift" "$src/Services/DaemonLifecycle.swift" \
  "$repo/tools/verification/daemon-lifecycle-check.swift" -o "$output/check"
"$output/check" "$output/pu-engine"

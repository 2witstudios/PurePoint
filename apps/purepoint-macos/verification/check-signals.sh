#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
task_dir=$(mktemp -d /tmp/pu-native-signals-XXXXXX)
trap 'rm -rf "$task_dir"' EXIT HUP INT TERM
mac="$repo/apps/purepoint-macos/purepoint-macos"
checks="$repo/apps/purepoint-macos/verification"
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
 "$mac/Models/AgentSignalPolicy.swift" "$mac/Models/AgentSignalSnapshot.swift" "$mac/Models/AgentSignalRouting.swift" "$checks/SignalPolicyChecks.swift" -o "$task_dir/policy"
"$task_dir/policy"
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
 "$mac/Models/AgentSignalPolicy.swift" "$mac/Models/AgentSignalSnapshot.swift" "$mac/Models/AgentSignalRouting.swift" \
 "$mac/Models/AgentStatus.swift" "$mac/Models/ManifestModel.swift" "$mac/Models/ChannelModel.swift" \
 "$mac/Services/DaemonProtocol.swift" "$mac/Services/DaemonConnection.swift" \
 "$checks/SignalWireChecks.swift" -o "$task_dir/mac-wire"
"$task_dir/mac-wire" "$repo"
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors -D MOBILE_SIGNAL_CHECKS \
 "$repo/apps/purepoint-mobile/PurePoint/ChatDomain.swift" \
 "$repo/apps/purepoint-mobile/PurePoint/AgentControlDomain.swift" "$checks/SignalWireChecks.swift" -o "$task_dir/phone-wire"
"$task_dir/phone-wire" "$repo"
swiftc -typecheck -default-isolation MainActor -strict-concurrency=complete -warnings-as-errors \
 "$mac/Models/AgentSignalPolicy.swift" "$mac/Models/AgentSignalSnapshot.swift" "$mac/Models/AgentSignalRouting.swift" "$mac/Models/AgentSignalPendingTaps.swift" "$mac/Services/AgentSignalNotifications.swift"
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
 "$repo/apps/purepoint-mobile/PurePoint/ChatDomain.swift" \
 "$repo/apps/purepoint-mobile/PurePoint/AgentControlDomain.swift" \
 "$repo/apps/purepoint-mobile/PurePoint/AgentControlModel.swift" \
 "$checks/SignalPhoneModelChecks.swift" -o "$task_dir/phone-model"
"$task_dir/phone-model" "$repo"

swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
 "$mac/Models/AgentSignalPolicy.swift" "$mac/Models/AgentSignalSnapshot.swift" "$mac/Models/AgentSignalRouting.swift" \
 "$checks/SignalRoutingChecks.swift" -o "$task_dir/routing"
"$task_dir/routing"

swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
 "$mac/Models/AgentSignalPendingTaps.swift" "$checks/SignalPendingTapChecks.swift" -o "$task_dir/pending-taps"
"$task_dir/pending-taps"

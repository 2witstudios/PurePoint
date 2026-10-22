#!/bin/sh
set -eu
module_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_dir=$(mktemp -d /tmp/pi-mobile-machines-XXXXXX)
trap 'rm -rf "$task_dir"' EXIT HUP INT TERM
cat "$module_root/PurePoint/ChatModel.swift" "$module_root/verification/MachineChecks.swift" > "$task_dir/MachineModel.swift"
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
  "$module_root/PurePoint/ChatDomain.swift" "$module_root/PurePoint/PairingSecret.swift" \
  "$module_root/PurePoint/AgentControlDomain.swift" "$module_root/PurePoint/AgentControlModel.swift" \
  "$task_dir/MachineModel.swift" -o "$task_dir/checks"
CFFIXED_USER_HOME="$task_dir" "$task_dir/checks"

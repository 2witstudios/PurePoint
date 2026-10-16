#!/bin/sh
set -eu
module_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_dir=$(mktemp -d /tmp/pi-mobile-navigation-XXXXXX)
trap 'rm -rf "$task_dir"' EXIT HUP INT TERM
swiftc -strict-concurrency=complete -warnings-as-errors "$module_root/PurePoint/ChatDomain.swift" "$module_root/PurePoint/AgentControlDomain.swift" "$module_root/verification/NavigationChecks.swift" -o "$task_dir/checks"
"$task_dir/checks"
python3 "$module_root/verification/check-xcode-project.py"

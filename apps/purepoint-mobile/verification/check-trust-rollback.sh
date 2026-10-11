#!/bin/sh
set -eu
module_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_dir=$(mktemp -d /tmp/pi-mobile-trust-rollback-XXXXXX)
trap 'rm -rf "$task_dir"' EXIT HUP INT TERM
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors \
  "$module_root/PurePoint/ChatDomain.swift" "$module_root/PurePoint/PairingSecret.swift" \
  "$module_root/verification/TrustRollbackChecks.swift" -o "$task_dir/checks"
"$task_dir/checks"

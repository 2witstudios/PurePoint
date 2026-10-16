#!/bin/sh
set -eu
module_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_dir=$(mktemp -d /tmp/pi-mobile-agent-control-XXXXXX)
trap 'rm -rf "$task_dir"' EXIT HUP INT TERM
swiftc -parse-as-library -strict-concurrency=complete -warnings-as-errors -D PHONE_CONTROL_CHECKS \
  "$module_root/PurePoint/ChatDomain.swift" "$module_root/PurePoint/AgentControlDomain.swift" \
  "$module_root/PurePoint/AgentControlModel.swift" "$module_root/PurePointTests/AgentControlScenarios.swift" \
  "$module_root/verification/AgentControlChecks.swift" -o "$task_dir/checks"
"$task_dir/checks"
# The list must remain alive as its navigation destination opens. Cleanup belongs
# to sheet dismissal or the agent detail leaving, never list disappearance.
python3 - "$module_root" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
source = (root/'PurePoint/AgentsView.swift').read_text()
list_view = source.split('private struct AgentListRow')[0]
assert '.onDisappear' not in list_view, 'Pushing the agent screen must not close its terminal'
assert 'onDismiss: { model.agentControl.closeScreen(); presentPendingDialog() }' in (root/'PurePoint/ChatView.swift').read_text()
print('Agent navigation lifecycle wiring checks passed')
PY

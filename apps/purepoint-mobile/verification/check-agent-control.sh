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
# Opening a workspace agent must use the same generation-fenced model/detail, and leaving
# the pushed agent screen (Back, Point Guard, lost selection) must close its stream.
python3 - "$module_root" <<'PYCODE'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
agents = (root/'PurePoint/AgentsView.swift').read_text()
chat = (root/'PurePoint/ChatView.swift').read_text()
assert 'AgentControlView(model: model, screen: model.screenToken)' in agents
assert '.onDisappear { model.closeScreen(screen: screen) }' in agents
assert '.onChange(of: model.selected) { _, selected in if selected == nil && visible { dismiss() } }' in agents
assert 'control.open(machine: machine, agent: agent, context: control.contextToken)' in chat
assert 'openPointGuard: { path = [.pointGuard] }' in chat
print('Agent navigation lifecycle wiring checks passed')
PYCODE

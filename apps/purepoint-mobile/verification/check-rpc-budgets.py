"""Compare the agents receipt deadline with the bridge's bounded fleet schedule."""
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
bridge = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "bridge"
terminals = (bridge / "terminals.js").read_text()
daemon = (bridge / "daemon.js").read_text()
workspace = (bridge / "workspace.js").read_text()
timing = (root / "PurePoint/ChatDomain.swift").read_text().split("enum MobileRPCTiming", 1)[1]
fleet = terminals.split('if (r.op === "agents") {', 1)[1].split('if (r.op === "terminal.open"', 1)[0]
limit = int(re.search(r"machines.length > (\d+)", fleet)[1])
batch = int(re.search(r"offset \+= (\d+)", fleet)[1])
timeouts = {int(value) for value in re.findall(r"timeout = (\d+)", daemon)}
assert len(timeouts) == 1, "Review daemon stages when per-request deadlines change"
request_ms = timeouts.pop()
inspection = daemon.split("const inspect = async", 1)[1].split("return { machines, resolve, inspect }", 1)[0]
inspection_stages = inspection.count("await daemonRequest(")
assert inspection_stages == 2, "Review health and agents inventory sequence"
metadata = workspace.split("export async function projectMetadata", 1)[1].split("const complete", 1)[0]
assert 'Promise.allSettled([' in metadata and 'query("projects")' in metadata and 'query("worktrees")' in metadata
batches = (limit + batch - 1) // batch
fleet_ms = batches * (inspection_stages + 1) * request_ms
# Missing an agents-specific case uses the generic 35-second deadline.
match = re.search(r'case "agents": return ([\d_]+)', timing)
agents_ms = int(match[1].replace("_", "")) // 1_000_000 if match else 35_000
assert agents_ms >= fleet_ms + 30_000, (agents_ms, fleet_ms, "Agents receipt needs transport/projection margin")
assert 'case "agent.prompt": return 390_000_000_000' in timing
assert 'default: return 35_000_000_000' in timing
print(f"RPC budget checks passed: {limit} machines / {batch} concurrent = {batches} batches; fleet {fleet_ms}ms, offline {batches * request_ms}ms, agents receipt {agents_ms}ms; prompt390s/generic35s unchanged")

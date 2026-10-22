"""Compare fleet and fixed workspace receipt deadlines with actual bridge stage caps."""
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
phone_ms = {operation: int(value.replace("_", "")) // 1_000_000 for operation, value in re.findall(r'case "([^"]+)": return ([\d_]+)', timing)}
# The exact fixed dispatcher must preserve the full confirmed-prompt window.
# These are operational socket caps; unusually slow daemon git/FS work may
# continue after timeout and must remain uncertain, never automatically replayed.
dispatch = re.search(r'timeout:\s*r\.op === "workspace.spawn"\s*\?\s*(\d+)\s*:\s*(\d+)', workspace)
assert dispatch, "Review operation-specific workspace dispatch caps"
spawn_dispatch_ms, short_dispatch_ms = map(int, dispatch.groups())
assert (spawn_dispatch_ms, short_dispatch_ms) == (360_000, 65_000)
catalog = workspace.split("const catalog = async", 1)[1].split("const request = async", 1)[0]
catalog_stages = catalog.count("await daemonRequest(")
assert catalog_stages == 2, "Review GetConfig/ListAgentDefs catalog preflight"
metadata_ms = request_ms  # Projects and Worktrees run in parallel above.
catalog_ms = catalog_stages * request_ms
inspect_ms = inspection_stages * request_ms
matrix = {
    "workspace.spawn": (metadata_ms + catalog_ms + spawn_dispatch_ms, 390_000),
    "workspace.terminal": (metadata_ms + catalog_ms + short_dispatch_ms, 100_000),
    "workspace.createWorktree": (metadata_ms + short_dispatch_ms, 100_000),
    "workspace.rename": (metadata_ms + inspect_ms + short_dispatch_ms, 100_000),
    "workspace.stop": (metadata_ms + inspect_ms + short_dispatch_ms, 100_000),
    "workspace.removeProject": (metadata_ms + inspect_ms + short_dispatch_ms, 100_000),
}
for operation, (bridge_ms, expected_phone_ms) in matrix.items():
    assert phone_ms.get(operation, 35_000) == expected_phone_ms, (operation, "Exact receipt deadline mismatch")
    assert expected_phone_ms >= bridge_ms + 15_000, (operation, bridge_ms, expected_phone_ms)
for operation in ["workspace.catalog", "workspace.operation", "sync"]:
    assert phone_ms.get(operation, 35_000) == 35_000, (operation, "Unrelated/read calls retain generic deadline")
assert not re.search(r'hasPrefix|startsWith', timing), "Do not widen arbitrary workspace operations"
assert 'case "agent.prompt": return 390_000_000_000' in timing
assert 'default: return 35_000_000_000' in timing
print("Workspace receipt matrix passed: " + "; ".join(f"{op} bridgewait{budget}ms/phone{phone}ms" for op, (budget, phone) in matrix.items()))
print(f"RPC budget checks passed: {limit} machines / {batch} concurrent = {batches} batches; fleet {fleet_ms}ms, offline {batches * request_ms}ms, agents receipt {agents_ms}ms; prompt390s/generic35s unchanged")

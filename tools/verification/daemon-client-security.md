# Client local-security verification

Baseline: `a79da2b2060d733fc46500f63b664202852acdb3`.

Run only standalone compilation and scratch fixtures:

```sh
sh tools/verification/check-daemon-state.sh
sh tools/verification/check-daemon-lifecycle.sh
CARGO_TARGET_DIR=/tmp/purepoint-mesh-client-lifecycle-target cargo test -p pu-core private_fs::tests -- --nocapture
CARGO_TARGET_DIR=/tmp/purepoint-mesh-client-lifecycle-target cargo test -p pu-cli daemon_ctrl::tests -- --nocapture
CARGO_TARGET_DIR=/tmp/purepoint-mesh-client-lifecycle-target cargo clippy -p pu-core -p pu-cli --all-targets -- -D warnings
cargo fmt --all -- --check
```

The ancestor regression was observed failing before the fix in both Swift
(`log created through public ancestor`) and CLI (`open_daemon_log` accepted the
unsafe ancestor). The new configurable lifecycle harness initially failed to
compile against the baseline's private launcher and missing owned-stop API.

The Swift lifecycle harness executes only a copied temporary Python fixture
named `pu-engine`, using private scratch state and Unix sockets. Fixtures record
requests and received signals, and exit themselves after eight seconds. No
fixture or real daemon is OS-signaled. It verifies:

- Unsafe state is rejected before probing even a healthy endpoint.
- A healthy shared daemon survives attach, quit and refused deliberate restart,
  even with old PID-file timestamps.
- Stale PID metadata naming a healthy process on another socket cannot authorize
  signals or shutdown while a fresh app child is launched and deliberately restarted.
- Health, project suspension and shutdown use one connected stream; replacing
  the pathname during health does not redirect mutations to the replacement listener.
- Replacing the endpoint before health verification rejects shutdown while the
  original launched child is alive.
- A replacement reporting an exited child's former PID cannot revive ownership.
- An acknowledged shutdown without actual child exit reports recovery uncertainty;
  it never authorizes escalation or replacement.
- BUSY, protocol mismatch, and accepted-but-mute endpoints fail within fixture
  deadlines without lifecycle mutation. Owned-child BUSY/mute shutdown also fails boundedly.

The Swift filesystem harness and Rust tests use real scratch files, directories,
links, hardlinks, FIFOs and Darwin ACLs. They check private creation/append, reject
unsafe ancestors and grant ACLs before creating missing logs/state, accept
root-owned temporary-directory aliases and deny-only ACLs, and preserve existing
unsafe entries and log contents. Shared Rust file-effect APIs validate bounded
basenames. `checked_path_for` is the preferred validated path constructor;
`path_for` remains compatible with existing engine callers using validated names.

`pu-core::private_fs` is extracted from the existing engine utility. This branch
does not edit the concurrently owned engine module; the parent must replace it
with an import/re-export during integration, then re-run engine lifecycle checks.
The utility performs filesystem validation, not locking or process supervision.

Full macOS application build and Xcode tests: **NOT RUN**. The lifecycle/client/
protocol/filesystem sources compile in the standalone Swift harness with Swift 5
and default MainActor isolation. AppState's small quit/watcher hooks require the
owner's full Xcode build. Linux-specific validation was not executed here.
Independent review and integration are parent-owned.

The boundary excludes other OS users. Same-account code and root remain trusted.
The retained `Process` plus compatible health on the mutation connection is
client ownership evidence within that model, not an OS authentication or
process-tree containment guarantee. There is no hard-kill fallback. Failed
suspension/shutdown can have partial effects or an uncertain acknowledgement;
recovery is explicit and no mutation is replayed automatically.

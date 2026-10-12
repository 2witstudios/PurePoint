# Running agents on another machine (LAN mesh)

One Point Guard ("host") can run and control agents on other machines on your local network ("workers"). Every worker keeps control of its own projects, runtimes and capacity. Nothing listens on the network until you set it up, and every connection is pinned to certificates you exchanged by hand.

Full contract: [LAN Mesh spec](../../product/lan-mesh.md).

## 1. Create identities (once per machine)

Run on both machines:

```sh
pu mesh identity initialize
pu mesh identity export --out ~/Desktop/$(hostname).pu-cert
```

`export` writes the public certificate and prints its **fingerprint**. Copy each certificate file to the other machine, then confirm the fingerprints match through a separate channel, such as reading them aloud or comparing on screen. Never accept a fingerprint that arrived together with the certificate.

## 2. On the worker: export a project and grant the host

The project must be a git repository initialized with `pu init`, and its `.pu/config.yaml` must configure the runtimes you will grant.

```sh
pu mesh export register /path/to/project --label my-project
pu mesh export list                 # note the export id
cat > grants.json <<'EOF'
[{"export_id": "<export id>",
  "capabilities": ["inspect", "read_output", "spawn", "control", "input", "stop"],
  "runtimes": ["claude"]}]
EOF
pu mesh enroll-manual --certificate-file host.pu-cert \
  --expected-fingerprint <host fingerprint> --grants-file grants.json
pu mesh config enable --control-address 192.168.1.20:7443 --bulk-address 192.168.1.20:7444
pu mesh listener                    # should report "listening"
```

Grant only what the host needs. Grants never include a shell, an executable or a path. Add `--expires-at <unix seconds>` for temporary access.

## 3. On the host: pin and route the worker

```sh
pu mesh enroll-manual --certificate-file worker.pu-cert \
  --expected-fingerprint <worker fingerprint> --grants-file empty.json   # empty.json: []
pu mesh worker add --fingerprint <worker fingerprint> --daemon-id <worker daemon id> \
  --label studio --control-address 192.168.1.20:7443 --bulk-address 192.168.1.20:7444
```

The host grants the worker nothing; enrolling the worker here only pins its certificate.

## 4. Run and control agents

```sh
pu mesh remote --worker studio inventory
pu mesh remote --worker studio spawn --export <id> --runtime claude \
  --commit <full commit sha> --prompt "Fix the flaky test"
pu mesh remote --worker studio status --export <id>
pu mesh remote --worker studio watch --export <id> --agent <agent id>
pu mesh remote --worker studio writer --export <id> --agent <agent id>
pu mesh remote --worker studio input --export <id> --agent <agent id> \
  --writer <n> --sequence 1 --text "continue" --submit
pu mesh remote --worker studio stop --export <id> --agent <agent id>
```

- **Exact source:** spawns use exactly the commit you name. If the worker doesn't have it, the spawn fails; there is no fallback.
- **Safe retries:** every mutation prints `--window N --operation ID`. If you aren't sure a mutation went through, rerun it with those same values; the worker returns the original receipt instead of acting twice.
- **No input replay:** each input sequence is delivered at most once, and its receipt says `written`, `partial`, `not_written` or `unknown`. After reconnecting, check `input-receipt`; nothing is resent for you.
- **Host restarts:** runs on the worker keep going when the host restarts or loses the network. Reconnect and keep controlling the same run.

## On the worker: your machine, your call

```sh
pu mesh run list                    # remote-owned runs and who controls them
pu mesh run takeover <agent id>     # cut off the remote controller; type into it locally
pu mesh run stop <agent id>
pu mesh peer revoke <fingerprint> --expected-generation <n>
pu mesh config disable
```

- **Local controls:** `pu kill`, `bench`, `play` and `rename` won't act on remote-owned runs. Use `pu mesh run takeover` first, then stop the run or type into it.
- **Revocation:** revoking a host cuts its access immediately, but the agent keeps running so you can decide what to do with it.
- **Daemon death:** if the worker daemon dies, the agent's whole process tree is stopped. After restart the run shows as `interrupted`.
- **Unknown runs:** a run marked `unknown` keeps its slot until you resolve it with `pu mesh run resolve <run id> --evidence "..."`. Only do that after checking that its processes are gone.

# Workflow package producer delivery

Builder: ag-z6w5838y · worktree wt-raqizid5 · branch pu/pointguard-package.
Parent: ag-3sz6gpfv · receiving branch pu/pointguard-soul-skills.
Owner-authorized scope: native Pointguard workflow package, pinned import tooling,
runtime packaging manifest and focused tooling tests. No PGSET/bridge/soul/pu skill
changes, source-checkout edits, app builds, installed-runtime changes or main merge.
The committed candidate is the git commit containing this record; obtain its full
SHA with `git rev-parse HEAD` after integration, and retain the producer SHA sent
through `pu send`. No producer PR; the parent owns the umbrella PR and acceptance.

## Independent parent-plan feedback

Read the parent's `tasks/pointguard-soul-skills-delivery.md`, CLAUDE.md, spec-system,
PGSET and distribution/desktop/module dependencies before implementation. Sent
actionable feedback to ag-3sz6gpfv using `pu --project <project-root> send`:

- Resolve the override as a native Pi package root, never recursively register its
  entire root as a skill directory (that would expose originals and adapters).
- Prove immutable source bytes/relative references and relocation; keep adapters
  separate and missing optional dependencies visible in useful native discovery.
- Preserve native system prompt/tool/skill sections in before_agent_start and prove
  new/resumed/compacted sessions; info/reload must report actual dependencies.
- Reconcile package manifest ownership: producer adds `paths.workflowPackage`,
  parent owns `support/manifest.json` and managed launch/override selection.

Plan is sound within the assigned ownership. These comments are proof obligations,
not an approval relay. Parent reports launch through `--extension <core package>
--extension <canonical workflow package>`, using native resolveExtensionSources.

## Implemented contract

- Package root: `support/pointguard-workflows`; `package.json` exposes only native
  `pi.skills: ["skills"]`. Fifteen adapters refer to original workflows; originals
  are not independently added to the native catalog. No parallel skill registry.
- `source/` is the full Pointguard tree at
  `a130ad906a0cac5e9183e5b9d0bf4b0b78f8921f`: 25 files, including all shared skills,
  codex plan/orchestrate, references, install.sh (executable), LICENSE and NOTICE.
- `source-manifest.json` records hashes/modes for provenance only. The importer reads
  committed git blobs, ignores checkout dirt, rejects unsupported tree entries and
  requires an explicit source checkout; no shipped developer home path.
- `PI-ADAPTERS.md` maps native Pi invocation and actual Codex/Claude, PageSpace
  CLI/SDK/MCP, AIDD and external-tool gaps. No auto-install, credential provisioning,
  fake tool execution, hidden skills or invented review/page identities.
- Runtime staging verifies imported sources/adapters before replacing output, copies
  support recursively and adds `runtime-manifest.json.paths.workflowPackage`.
- Parent selects exactly one canonical bundled/override package root using
  `POINT_GUARD_WORKFLOW_SOURCE`; this package does not interpret the environment.

## Reproduction and evidence

From the receiving repository root (supply your own explicit source/Pi locations):

```sh
POINTGUARD_TEST_SOURCE=/path/to/pointguard python3 apps/purepoint-mobile/runtime/test-pointguard-workflows.py
node apps/purepoint-mobile/support/pointguard-workflows/tools/test-native-loading.mjs /path/to/node_modules/@earendil-works/pi-coding-agent
python3 apps/purepoint-mobile/runtime/test-package-runtime.py
git diff --check
```

PASS: four Python tests: every pinned git blob/mode and complete inventory matches;
regeneration matches the committed package and leaves source checkout status intact;
relocated verification succeeds and detects tampering; absent pin refuses replacement;
actual stage/copy/manifest logic retains all package resources after relocation.
The stage test substitutes Node/helper bytes, npm dependency creation and native
graph completion. It proves package/resource staging, not executable app acceptance.

PASS: real installed Pi 1.1.0 DefaultPackageManager and DefaultResourceLoader resolve
the native manifest with clean HOME/stripped PATH; all 15 workflows remain visible,
with useful dependency notices. Adapter references resolve after relocation. Native
progressive catalog excludes workflow bodies. Reload, duplicate package identity,
symlink skill dedup, canonical local override and distinct same-name user skill
collision diagnostics pass. No credentials, model calls or network are needed.

PASS: existing interrupted-native-download cleanup/exact-integrity retry regression.
PASS: `git diff --check`.
RED evidence: initial source integrity test failed because the package provenance
was absent before implementation. All current focused tests pass.

## Remaining proof / integration instructions

Parent integrates the exact producer SHA into its allocated unprotected receiving
branch after its live protection check; no changes in another writer's checkout.
Producer edits only package-runtime.py plus the new package and focused test file.
Parent should add the workflow package to support/manifest.json if that inventory is
used, preserve `paths.workflowPackage` when composing any packaging changes, and
run the commands above against the composed commit. This record stays truthful until
the parent pins the composed candidate and independent review.

NOT RUN here: composed managed RPC launch/extension info/reload, prompt/session/
compaction/cancellation proofs (parent owns bridge/extension); genuine relocated app
executables/architecture/signature proof and human Xcode/device acceptance; PageSpace,
GitHub, AIDD/browser/desktop account/integration execution. These remain parent/main
acceptance obligations where applicable. Missing integrations are deliberately
reported at use. No independent producer code reviewer spawned; parent is obtaining
the umbrella exact-head review, avoiding redundant workers. No main readiness,
production activation or Done claim is made.

Security review: no new listeners/auth/credential operations (access control,
cryptographic failures, authentication, SSRF inapplicable); subprocess arguments use
lists and pinned git blobs (injection); no auto-provisioning (insecure design/security
misconfiguration); upstream pinned revision and complete hashes/modes (components/
software integrity); records contain no tokens (logging). Existing source prose is
preserved, with host limitations explicit. Protected production boundaries remain.

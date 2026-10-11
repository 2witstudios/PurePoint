# Pointguard workflows for Pi

An independently usable native Pi package. All shared workflows and Codex
plan/orchestrate come from Pointguard commit
`a130ad906a0cac5e9183e5b9d0bf4b0b78f8921f`. `source/` preserves the complete
pinned tree, including references, scripts and attribution. `skills/` contains
small Pi adapters; the original workflows remain the source of truth.

Install the local package with `pi install /absolute/path/to/pointguard-workflows`
or add that absolute package root to Pi's native `packages` settings. No npm install
is needed for this package. Native Pi discovers all 15 workflows progressively
through `pi.skills`; optional missing dependencies do not hide skills. See
[PI-ADAPTERS.md](PI-ADAPTERS.md) for truthful capability limits and useful fallbacks.
This package does not bundle PurePoint CLI skills, PageSpace, GitHub, AIDD, browser
tools or credentials.

The app bundles this package at `support/pointguard-workflows`. Its managed runtime
may select a developer package root through `POINT_GUARD_WORKFLOW_SOURCE`; select
one package source instead of registering both bundled and override copies.
That environment variable is an app contract, not a native Pi environment variable.

To regenerate from an explicit source checkout containing the pinned git commit:

```sh
python3 tools/import-workflows.py --source /path/to/pointguard
python3 tools/import-workflows.py --verify
```

Run these commands from this package directory. Regeneration reads committed git
blobs, ignoring dirty/untracked source files, and never edits the source checkout.
No developer home path is shipped. Use `--output /path/to/scratch-package` to compare
generated source, adapters and provenance without changing this package. Handwritten
package metadata and PI-ADAPTERS.md are maintained separately. `source-manifest.json`
is an integrity inventory, not a runtime skill registry.

Focused Python proofs live at `../../runtime/test-pointguard-workflows.py` in the
repository. Native Pi loading/relocation proof (pass the installed Pi 1.1.0 root):

```sh
node tools/test-native-loading.mjs /path/to/node_modules/@earendil-works/pi-coding-agent
```

Licensing: upstream MIT license and AIDD attribution are preserved in both the
package root and source tree. Adapters/tooling are MIT, copyright 2026 2witstudios.

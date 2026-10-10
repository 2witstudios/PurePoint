# Bundled Pi verification — 2026-10-10

Point Guard owns its Pi runtime. Users install and update PurePoint; they do not need to install or update Node, npm, or Pi separately. The app manifest pins Pi 1.1.0 and Node 22.23.3. The native owner launches the explicit bundled Node helper; the bridge launches that same executable with its bundled Pi adapter and package. Canonical Pi credentials and sessions remain outside the app so app updates preserve them.

Managed RPC startup/exit recovery now directs users to Point Guard setup and PurePoint retry/update/reinstallation. Development bridge startup retains npm guidance. Missing-model feedback asks for provider/model configuration without requiring a separate terminal Pi installation.

Verification on this Mac used the installed v0.4 fbd3f18 bundle resources copied to a disposable directory, the original app's bundled Node/pu/lock helpers read-only, a new empty HOME and PATH=/no-external-tools. The permissions candidate 110e9e4 was overlaid only into the disposable resource copy. The runtime proof passed native SDK credential prompt/completion with a dummy noncredential (no model request), clean startup, existing 0755 Pi-directory compatibility, session/auth/trust persistence across resource replacement, process crash recovery, helper-loss teardown, and exclusion of a concurrent writer. No real user credentials or installed app files were changed.

This proves no separately installed Node or Pi is needed for the exercised startup/setup path. It does not prove relocated executable signing, both CPU architectures, Developer ID/notarization, actual provider OAuth, or paid model execution. The unmodified relocated-artifact proof failed locally because its copied Node helper was SIGKILLed even for --version; the installed original ran successfully. That signing/relocation acceptance remains separate.

Scoped recovery regression checks: 57 RPC/controller tests passed, including real missing-executable and child-exit cases. Managed messages no longer direct the user to npm or a terminal; developer guidance remains.

import test from "node:test";
import assert from "node:assert/strict";
import { startupFailure } from "./runtime-errors.js";
test("given startup failure classes should give safe actionable attributed recovery", () => {
  for (const [stage, message, code] of [
    ["trust", "expired secret", "identity_expired"],
    ["trust", "mismatched secret", "identity_corrupt"],
    ["runtime", "secret", "missing_runtime"],
    ["cwd", "secret", "invalid_cwd"],
    ["capabilities", "secret", "local_capability_state"],
    ["credentials", "secret", "credential_state"],
    ["lock", "already running secret", "startup_collision"],
    ["lock", "corrupt state secret", "state_corrupt"],
    ["lock", "must be private secret", "private_state"],
  ]) {
    const r = startupFailure(new Error(message), stage, "instance", 123);
    assert.equal(r.code, code);
    assert.equal(r.instanceId, "instance");
    assert.equal(r.pid, 123);
    assert.equal(JSON.stringify(r).includes("secret"), false);
    assert.ok(r.recovery.length > 30);
  }
});

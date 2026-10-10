import test from "node:test";
import assert from "node:assert/strict";
import { optionalRemoteListener } from "./remote-listener.js";
test("given an unavailable remote interface should preserve only sanitized native recovery", async () => {
  for (const code of ["EADDRNOTAVAIL", "ENETUNREACH", "EHOSTUNREACH"]) {
    const result = await optionalRemoteListener(async () => {
      throw Object.assign(new Error("private credential"), { code });
    });
    assert.equal(result.server, undefined);
    assert.equal(result.remoteRecovery.code, "remote_unavailable");
    assert.equal(JSON.stringify(result).includes("private credential"), false);
  }
});
test("given collision or identity failure should never degrade or replace the configured endpoint", async () => {
  for (const code of ["EADDRINUSE", "EACCES", "IDENTITY_CORRUPT", undefined]) {
    const error = Object.assign(new Error("failure"), { code });
    await assert.rejects(
      optionalRemoteListener(async () => {
        throw error;
      }),
      (e) => e === error,
    );
  }
  const server = {};
  assert.equal(
    (await optionalRemoteListener(async () => server)).server,
    server,
  );
});

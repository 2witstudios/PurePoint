import test from "node:test";
import assert from "node:assert/strict";
import { serveAdmin } from "./admin.js";
test("given a phone credential or browser origin should reject local administrative access", async () => {
  let called = 0;
  const server = await serveAdmin({
    token: "owner-token",
    dispatch: async () => {
      called++;
      return { phase: "ready" };
    },
  });
  try {
    const request = (token, extra = {}) =>
      fetch(server.url, {
        method: "POST",
        headers: {
          authorization: `Bearer ${token}`,
          "content-type": "application/json",
          ...extra,
        },
        body: JSON.stringify({ operation: "status" }),
      });
    assert.equal((await request("phone-token")).status, 401);
    assert.equal(
      (await request("owner-token", { origin: "https://attacker.example" }))
        .status,
      403,
    );
    assert.equal((await fetch(server.url)).status, 405);
    assert.equal(called, 0);
    const response = await request("owner-token");
    assert.deepEqual(await response.json(), {
      ok: true,
      result: { phase: "ready" },
    });
    assert.equal(called, 1);
  } finally {
    await server.shutdown();
  }
});
test("given an oversized admin body or raw SDK error should reject and avoid disclosure", async () => {
  const server = await serveAdmin({
    token: "owner-token",
    dispatch: async () => {
      throw new Error("secret-api-key");
    },
  });
  try {
    const response = await fetch(server.url, {
      method: "POST",
      headers: {
        authorization: "Bearer owner-token",
        "content-type": "application/json",
      },
      body: JSON.stringify({ operation: "status" }),
    });
    assert.equal(
      JSON.stringify(await response.json()).includes("secret-api-key"),
      false,
    );
    const oversized = await fetch(server.url, {
      method: "POST",
      headers: {
        authorization: "Bearer owner-token",
        "content-type": "application/json",
      },
      body: " ".repeat(65537),
    });
    assert.equal(oversized.status, 413);
  } finally {
    await server.shutdown();
  }
});

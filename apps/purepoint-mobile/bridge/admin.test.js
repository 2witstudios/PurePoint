import test from "node:test";
import assert from "node:assert/strict";
import { serveAdmin, setDeviceTerminals } from "./admin.js";
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

test("owner permission administration accepts only off/view/control and rejects phone authority", async () => {
  const calls = [];
  const trust = {
    setTerminals(deviceId, terminals) {
      calls.push({ deviceId, terminals });
      return {};
    },
  };
  const server = await serveAdmin({
    token: "owner-token",
    dispatch: async (r) => {
      assert.equal(r.operation, "devices.terminals");
      return setDeviceTerminals(trust, r);
    },
  });
  const change = (terminals, token = "owner-token") =>
    fetch(server.url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        operation: "devices.terminals",
        deviceId: "phone",
        terminals,
      }),
    });
  try {
    assert.equal((await change("control", "phone-token")).status, 401);
    assert.equal(calls.length, 0);
    for (const permission of ["off", "view", "control"]) {
      assert.deepEqual(await (await change(permission)).json(), {
        ok: true,
        result: {},
      });
    }
    const invalid = await change("admin");
    assert.equal(invalid.status, 400);
    assert.equal((await invalid.json()).error.code, "terminal_permission");
    assert.deepEqual(
      calls,
      ["off", "view", "control"].map((terminals) => ({
        deviceId: "phone",
        terminals,
      })),
    );
  } finally {
    await server.shutdown();
  }
});

test("owner workspace permission accepts only explicit boolean grant", async () => {
  const { setDeviceWorkspaceManagement } = await import("./admin.js");
  const calls = [];
  const trust = {
    setWorkspaceManagement: async (...args) => {
      calls.push(args);
      return {};
    },
  };
  for (const workspaceManagement of [undefined, "true", 1, null])
    assert.throws(() =>
      setDeviceWorkspaceManagement(trust, {
        deviceId: "phone",
        workspaceManagement,
      }),
    );
  await setDeviceWorkspaceManagement(trust, {
    deviceId: "phone",
    workspaceManagement: true,
  });
  assert.deepEqual(calls, [["phone", true]]);
});

import test from "node:test";
import assert from "node:assert/strict";
import { AuthStorage } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
import { ProviderSetup } from "./provider.js";
const tick = () => new Promise((resolve) => setImmediate(resolve));
function setup(login, options = {}) {
  const credentials = AuthStorage.inMemory();
  const providers = [
    { id: "one", name: "One", auth: { oauth: { login } } },
    {
      id: "two",
      name: "Two",
      auth: {
        apiKey: { login: async () => ({ type: "api_key", key: "new-secret" }) },
      },
    },
  ];
  return {
    credentials,
    api: new ProviderSetup({ credentials, providers, models: [], ...options }),
  };
}
test("given canceled OAuth completing late should preserve newer credentials", async () => {
  let finish;
  const { credentials, api } = setup(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const old = api.start("one", "oauth");
  await tick();
  api.cancel(old.attemptId);
  await credentials.modify("one", async () => ({
    type: "api_key",
    key: "new-secret",
  }));
  finish({
    type: "oauth",
    access: "old-secret",
    refresh: "old-refresh",
    expires: Date.now() + 100000,
  });
  await tick();
  assert.equal((await credentials.read("one")).key, "new-secret");
  assert.equal(api.status(old.attemptId).status, "canceled");
  assert.equal(
    JSON.stringify(api.status(old.attemptId)).includes("secret"),
    false,
  );
  api.close();
});
test("given expired login completing late should not persist credential or leak provider errors", async () => {
  let now = 0,
    finish;
  const { credentials, api } = setup(
    () => new Promise((resolve) => (finish = resolve)),
    { now: () => now, ttlMs: 100 },
  );
  const attempt = api.start("one", "oauth");
  await tick();
  now = 101;
  assert.equal(api.status(attempt.attemptId).status, "expired");
  finish({
    type: "oauth",
    access: "secret",
    refresh: "refresh",
    expires: 10000,
  });
  await tick();
  assert.equal(await credentials.read("one"), undefined);
  api.close();
});
test("given provider changes should cancel pending prompt and reject stale answers", async () => {
  const { credentials, api } = setup(async (interaction) => {
    const key = await interaction.prompt({
      type: "secret",
      message: "Enter key",
    });
    return { type: "api_key", key };
  });
  const first = api.start("one", "oauth");
  await tick();
  const prompt = api.status(first.attemptId).prompt;
  const second = api.start("two", "api_key");
  await tick();
  assert.throws(() => api.respond(first.attemptId, prompt.id, "late-secret"));
  assert.equal(api.status(first.attemptId).status, "canceled");
  assert.equal(api.status(second.attemptId).status, "complete");
  assert.equal(await credentials.read("one"), undefined);
  api.close();
});
test("given SDK prompt-specific abort should clear prompt and reject stale response", async () => {
  const controller = new AbortController();
  const { api } = setup(async (interaction) => {
    await interaction.prompt({
      type: "manual_code",
      message: "Code",
      signal: controller.signal,
    });
  });
  const a = api.start("one", "oauth");
  await tick();
  const prompt = api.status(a.attemptId).prompt;
  controller.abort();
  await tick();
  assert.equal(api.status(a.attemptId).prompt, undefined);
  assert.throws(() => api.respond(a.attemptId, prompt.id, "code"));
  api.close();
});
test("given a credential changed outside app while OAuth is pending should preserve native newer auth", async () => {
  let finish;
  const { credentials, api } = setup(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const attempt = api.start("one", "oauth");
  await tick();
  await credentials.modify("one", async () => ({
    type: "api_key",
    key: "native-new-secret",
  }));
  finish({
    type: "oauth",
    access: "late-secret",
    refresh: "late-refresh",
    expires: 100000,
  });
  await tick();
  assert.equal((await credentials.read("one")).key, "native-new-secret");
  assert.equal(api.status(attempt.attemptId).status, "failed");
  api.close();
});
test("given provider throws credential-bearing error should return only sanitized failure", async () => {
  const { api } = setup(async () => {
    throw new Error("provider key secret-leak");
  });
  const attempt = api.start("one", "oauth");
  await tick();
  assert.equal(api.status(attempt.attemptId).status, "failed");
  assert.equal(
    JSON.stringify(api.status(attempt.attemptId)).includes("secret-leak"),
    false,
  );
  api.close();
});
test("given unchanged native key expression should compare raw snapshot and permit explicit new login", async () => {
  const credentials = AuthStorage.inMemory({
    one: { type: "api_key", key: "$PRPG_FAKE_KEY" },
  });
  const api = new ProviderSetup({
    credentials,
    providers: [
      {
        id: "one",
        auth: {
          apiKey: {
            login: async () => ({ type: "api_key", key: "replacement-proof" }),
          },
        },
      },
    ],
    models: [],
  });
  const a = api.start("one", "api_key");
  await tick();
  assert.equal(api.status(a.attemptId).status, "complete");
  assert.equal((await credentials.read("one")).key, "replacement-proof");
  api.close();
});
test('given OAuth returns an expired token should preserve the prior credential',async()=>{
 const {api,credentials}=setup(async()=>({type:'oauth',access:'expired-token',refresh:'expired-refresh',expires:1}));
 await credentials.modify('one',async()=>({type:'api_key',key:'existing-proof'}));
 const attempt=api.start('one','oauth');await tick();assert.equal(api.status(attempt.attemptId).status,'failed');
 assert.equal((await credentials.read('one')).key,'existing-proof');api.close();
});

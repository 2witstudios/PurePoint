import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { SessionManager } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js";
import { installManagedPersistence } from "./native-persistence.js";
test("given managed native setup with no prompt should persist native tree and restore its exact session without fake messages", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pi-managed-persist-"));
  try {
    installManagedPersistence(SessionManager);
    const manager = SessionManager.create(dir, path.join(dir, "sessions"));
    manager.appendModelChange("anthropic", "model");
    const file = manager.getSessionFile();
    const entries = (await readFile(file, "utf8"))
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));
    assert.equal(entries[0].id, manager.getSessionId());
    assert.equal(
      entries.some((e) => e.type === "message"),
      false,
    );
    const restored = SessionManager.open(file);
    assert.equal(restored.getSessionId(), manager.getSessionId());
    assert.equal(restored.getLeafId(), manager.getLeafId());
    assert.equal((await stat(file)).mode & 0o777, 0o600);
    manager.appendMessage({
      role: "user",
      content: "native persisted prompt",
      timestamp: 1,
    });
    assert.equal(
      (await readFile(file, "utf8")).split("native persisted prompt").length,
      2,
    );
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

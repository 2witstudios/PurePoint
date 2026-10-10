import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { openTrustStore } from "./trust.js";
const execute = promisify(execFile);
test("expired persisted TLS identity fails actionably without rotating certificate or host trust", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pg-tls-expiry-"));
  let store;
  try {
    store = await openTrustStore({ directory });
    await store.close();
    const certFile = path.join(directory, "identity-cert.pem");
    const keyFile = path.join(directory, "identity-key.pem");
    const originalState = await readFile(path.join(directory, "trust.json"));
    // Issue an expired certificate only inside this disposable fixture.
    await writeFile(path.join(directory, "index.txt"), "");
    await writeFile(path.join(directory, "serial"), "01");
    await writeFile(
      path.join(directory, "ca.cnf"),
      `[ca]\ndefault_ca=test\n[test]\ndatabase=${directory}/index.txt\nserial=${directory}/serial\nnew_certs_dir=${directory}\ncertificate=${certFile}\ndefault_md=sha256\npolicy=policy\n[policy]\ncommonName=supplied\n`,
    );
    await execute("/usr/bin/openssl", [
      "req",
      "-new",
      "-key",
      keyFile,
      "-subj",
      "/CN=Point Guard",
      "-out",
      path.join(directory, "request.pem"),
    ]);
    await execute("/usr/bin/openssl", [
      "ca",
      "-batch",
      "-selfsign",
      "-keyfile",
      keyFile,
      "-in",
      path.join(directory, "request.pem"),
      "-startdate",
      "20200101000000Z",
      "-enddate",
      "20200102000000Z",
      "-config",
      path.join(directory, "ca.cnf"),
      "-out",
      path.join(directory, "expired.pem"),
    ]);
    const expired = await readFile(path.join(directory, "expired.pem"), "utf8");
    await writeFile(certFile, expired);
    await assert.rejects(openTrustStore({ directory }), /expired|invalid/i);
    assert.equal(await readFile(certFile, "utf8"), expired);
    assert.deepEqual(
      await readFile(path.join(directory, "trust.json")),
      originalState,
    );
  } finally {
    await store?.close();
    await rm(directory, { recursive: true, force: true });
  }
});

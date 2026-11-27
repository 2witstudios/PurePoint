import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  daemonEndpoint,
  hash16,
  normalizeSocketPath,
  normalizeText,
  pipeName,
  pipeNameForSocket,
} from "./pipe-name.js";

// Printed by `pu_transport::naming::pipe_name` (crates/pu-transport/src/naming.rs)
// for these normalized paths; the JS port must match them byte for byte.
const RUST_VECTORS = [
  [
    String.raw`C:\Users\x\.pu\daemon.sock`,
    String.raw`\\.\pipe\purepoint-3bc8db789fd89339-daemon`,
  ],
  [
    String.raw`C:\Users\Jane Doe\.pu\machines\m1\daemon.sock`,
    String.raw`\\.\pipe\purepoint-634aafccd4ab199d-daemon`,
  ],
  [
    String.raw`C:\Users\Zoë\.pu\daemon.sock`,
    String.raw`\\.\pipe\purepoint-26bec7559f76c3b3-daemon`,
  ],
  [
    String.raw`D:\a\_temp\home\.pu\daemon.sock`,
    String.raw`\\.\pipe\purepoint-e7413f9cfb3f861d-daemon`,
  ],
];

test("given the Rust vectors, should name the same daemon pipes", () => {
  for (const [normalized, expected] of RUST_VECTORS)
    assert.equal(pipeName(normalized, "daemon"), expected, normalized);
});

test("given another role, should keep the hash and change the suffix", () => {
  const owner = String.raw`C:\Users\x\AppData\Local\PurePoint\point-guard\admin\admin.sock`;
  assert.equal(
    pipeName(owner, "pointguard-owner"),
    String.raw`\\.\pipe\purepoint-ba940eb4bfa2349d-pointguard-owner`,
  );
  assert.equal(hash16(owner), "ba940eb4bfa2349d");
});

test("given text, should normalize drive and separators like normalize_text", () => {
  assert.equal(normalizeText("c:/Users/x/"), String.raw`C:\Users\x`);
  assert.equal(normalizeText("C:\\"), "C:\\");
  assert.equal(normalizeText("c:\\\\"), "C:\\");
  assert.equal(
    normalizeText(String.raw`C:\Users\Mixed\Case`),
    String.raw`C:\Users\Mixed\Case`,
  );
});

test("given a Unix host, should connect to the socket path unchanged", () => {
  assert.equal(
    daemonEndpoint("/home/x/.pu/daemon.sock", "linux"),
    "/home/x/.pu/daemon.sock",
  );
  assert.equal(
    daemonEndpoint("/Users/x/.pu/daemon.sock", "darwin"),
    "/Users/x/.pu/daemon.sock",
  );
});

const windows = { skip: process.platform !== "win32" && "Windows paths" };

test("given spellings of one path, should name one pipe", windows, () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "pp-pipe-"));
  try {
    const pu = path.join(home, ".pu");
    mkdirSync(pu);
    const plain = pipeNameForSocket(path.join(pu, "daemon.sock"));
    assert.match(plain, /^\\\\\.\\pipe\\purepoint-[0-9a-f]{16}-daemon$/);
    for (const v of [
      `${pu.replaceAll("\\", "/")}/daemon.sock`,
      `${pu}\\..\\.pu\\daemon.sock`,
      `${pu.toUpperCase()}\\daemon.sock`,
    ])
      assert.equal(pipeNameForSocket(v), plain, v);
    const link = path.join(home, "junction");
    symlinkSync(pu, link, "junction");
    assert.equal(pipeNameForSocket(path.join(link, "daemon.sock")), plain);
    assert.notEqual(pipeNameForSocket(path.join(pu, "other.sock")), plain);
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
});

test(
  "given a missing parent, should create it like the Rust side",
  windows,
  () => {
    const home = mkdtempSync(path.join(os.tmpdir(), "pp-pipe-"));
    try {
      const socket = path.join(home, "machines", "m1", "daemon.sock");
      const normalized = normalizeSocketPath(socket);
      // Canonical (long-name, on-disk) parent: CI temp dirs are 8.3 names.
      const canonical = path.join(
        realpathSync.native(home),
        "machines",
        "m1",
        "daemon.sock",
      );
      assert.equal(normalized, canonical);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  },
);

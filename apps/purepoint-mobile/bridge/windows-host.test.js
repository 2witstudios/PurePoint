import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { request } from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  currentUserSid,
  grantsWrite,
  writableByOthers,
  isPrivateAcl,
  makePrivate,
  parseSavedAcl,
  readAcl,
} from "./acl.js";
import { serveAdmin } from "./admin.js";
import { daemonRequest } from "./daemon.js";
import {
  hostCopy,
  hostLabel,
  ownerPipeName,
  platformDefaults,
} from "./platform.js";
import { pipeNameForSocket } from "./pipe-name.js";
import { privateDirectory, privatePath } from "./runtime-state.js";
import { startRemote } from "./remote.js";
import {
  discoverTailnet,
  serveCommands,
  tailscalePaths,
  windowsTailscalePaths,
} from "./tailnet.js";

const windows = { skip: process.platform !== "win32" && "Windows host only" };
const USER = "S-1-5-21-1-2-3-1001";
const icacls = path.win32.join(
  process.env.SystemRoot ?? "C:\\Windows",
  "System32",
  "icacls.exe",
);

test("platform defaults: LocalAppData state and an owner pipe on Windows", () => {
  const win = platformDefaults({
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\Users\\u\\AppData\\Local" },
    home: "C:\\Users\\u",
  });
  assert.equal(
    win.stateDirectory,
    "C:\\Users\\u\\AppData\\Local\\PurePoint\\point-guard",
  );
  assert.equal(win.adminTransport, "pipe");
  assert.equal(win.hostKind, "windows");
  // The app passes PI_MOBILE_HOST; only headless `pu phone` asks to discover.
  assert.equal(win.discoverTailnet, false);
  assert.equal(
    platformDefaults({
      platform: "win32",
      env: { POINT_GUARD_DISCOVER_TAILNET: "1" },
      home: "C:\\Users\\u",
    }).discoverTailnet,
    true,
  );
  // A missing or relative LOCALAPPDATA falls back under the profile, never cwd.
  for (const env of [{}, { LOCALAPPDATA: "Local" }])
    assert.equal(
      platformDefaults({ platform: "win32", env, home: "C:\\Users\\u" })
        .stateDirectory,
      "C:\\Users\\u\\AppData\\Local\\PurePoint\\point-guard",
    );
  assert.equal(hostLabel("…", "windows"), "windows-pc");
  assert.equal(hostLabel("DESKTOP-1.tail.ts.net", "windows"), "DESKTOP-1");
  assert.equal(hostCopy("windows").host, "this PC");
});

test("saved ACL parsing: protected owner-only DACLs pass, anything wider fails", () => {
  const own = parseSavedAcl(
    `e\r\nD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;${USER})\r\n`,
  );
  assert.equal(own.protected, true);
  assert.deepEqual(
    own.aces.map((a) => a.sid),
    ["S-1-5-18", USER],
  );
  assert.equal(isPrivateAcl(own, USER, { directory: true }), true);
  const admins = parseSavedAcl(
    `\uFEFFe\r\nD:PAI(A;OICI;FA;;;${USER})(A;OICI;0x1200a9;;;BA)\r\n`,
  );
  assert.equal(isPrivateAcl(admins, USER, { directory: true }), true);
  const cases = {
    inherited: `D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;${USER})`,
    everyone: `D:PAI(A;OICI;FA;;;${USER})(A;OICI;FR;;;WD)`,
    users: `D:PAI(A;OICI;FA;;;${USER})(A;OICI;FR;;;BU)`,
    other: `D:PAI(A;OICI;FA;;;S-1-5-21-1-2-3-1002)`,
    creatorOwner: `D:PAI(A;OICI;FA;;;${USER})(A;OICIIO;FA;;;CO)`,
    inheritedEntry: `D:PAI(A;OICI;FA;;;${USER})(A;OICIID;FA;;;SY)`,
  };
  for (const [name, sddl] of Object.entries(cases))
    assert.equal(
      isPrivateAcl(parseSavedAcl(`d\r\n${sddl}\r\n`), USER, {
        directory: true,
      }),
      false,
      name,
    );
  // Files inside a private directory inherit its entries.
  const file = parseSavedAcl(
    `f.json\r\nD:AI(A;ID;FA;;;SY)(A;ID;FA;;;${USER})\r\n`,
  );
  assert.equal(isPrivateAcl(file, USER), true);
  assert.throws(() => parseSavedAcl("garbage"), /Unreadable/);
});

test(
  "given a state directory, should replace its ACL with user + SYSTEM only",
  windows,
  async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "pp-acl-"));
    try {
      const dir = path.join(root, "PurePoint", "point-guard");
      await privateDirectory(dir);
      const sid = await currentUserSid();
      const acl = await readAcl(dir);
      assert.equal(acl.protected, true);
      assert.deepEqual(
        new Set(acl.aces.map((a) => a.sid)),
        new Set([sid, "S-1-5-18"]),
      );
      assert.ok(acl.aces.every((a) => a.rights === "FA" && a.flags === "OICI"));
      // icacls' own listing (names are localized; the flags are not).
      const listing = execFileSync(icacls, [dir], {
        encoding: "utf8",
        windowsHide: true,
      });
      const entries = listing
        .split(/\r?\n/)
        .map((l) => l.replace(dir, "").trim())
        .filter((l) => /:\(/.test(l));
      assert.equal(entries.length, 2, listing);
      for (const entry of entries) {
        assert.match(entry, /:\(OI\)\(CI\)\(F\)$/, entry);
        assert.doesNotMatch(entry, /\(I\)/, entry);
      }
      // A file written inside inherits the private entries and passes the check.
      const file = path.join(dir, "runtime.json");
      writeFileSync(file, "{}\n");
      await privatePath(file);
      await privatePath(dir, true);
      // Re-running on an existing directory keeps it private.
      await privateDirectory(dir);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);

test(
  "given a widened directory, should refuse it as private state",
  windows,
  async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "pp-acl-"));
    try {
      const dir = path.join(root, "state");
      mkdirSync(dir);
      // Inherited temp ACL: not protected.
      await assert.rejects(privatePath(dir, true), /private/);
      await makePrivate(dir);
      execFileSync(icacls, [dir, "/grant", "*S-1-1-0:(OI)(CI)R"], {
        windowsHide: true,
      });
      await assert.rejects(privatePath(dir, true), /private/);
      await assert.rejects(makePrivate(dir), /private/);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);

const post = (socketPath, token, body = { operation: "status" }) =>
  new Promise((resolve, reject) => {
    const payload = JSON.stringify(body);
    const req = request(
      {
        socketPath,
        path: "/admin/v1",
        method: "POST",
        headers: {
          ...(token ? { authorization: `Bearer ${token}` } : {}),
          "content-type": "application/json",
          "content-length": Buffer.byteLength(payload),
        },
      },
      (res) => {
        let text = "";
        res.on("data", (c) => (text += c));
        res.on("end", () =>
          resolve({ status: res.statusCode, body: JSON.parse(text) }),
        );
      },
    );
    req.on("error", reject);
    req.end(payload);
  });

test(
  "given the owner pipe, should require the bearer token",
  windows,
  async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "pp-owner-"));
    let called = 0;
    const pipeName = ownerPipeName(root);
    assert.match(
      pipeName,
      /^\\\\\.\\pipe\\purepoint-[0-9a-f]{16}-pointguard-owner$/,
    );
    const admin = await serveAdmin({
      token: "owner-token",
      pipeName,
      dispatch: async () => {
        called++;
        return { phase: "ready" };
      },
    });
    try {
      assert.equal(admin.socket, pipeName);
      assert.equal((await post(pipeName, undefined)).status, 401);
      assert.equal((await post(pipeName, "phone-token")).status, 401);
      assert.equal(called, 0);
      assert.deepEqual((await post(pipeName, "owner-token")).body, {
        ok: true,
        result: { phase: "ready" },
      });
      assert.equal(called, 1);
      // First-instance create: a second owner on the same pipe fails closed.
      await assert.rejects(
        serveAdmin({ token: "x", pipeName, dispatch: async () => ({}) }),
        /EADDRINUSE|EACCES/,
      );
    } finally {
      await admin.shutdown();
      rmSync(root, { recursive: true, force: true });
    }
    await assert.rejects(
      serveAdmin({
        token: "x",
        pipeName: "\\\\.\\pipe\\evil",
        dispatch: async () => ({}),
      }),
      /pipe name/,
    );
  },
);

// Integration: the workspace-built daemon (`cargo build -p pu-engine`).
const repo = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
const engine =
  process.env.PU_ENGINE_PATH ??
  ["debug", "release"]
    .map((p) => path.join(repo, "target", p, "pu-engine.exe"))
    .find((p) => existsSync(p));

test(
  "given pu-engine.exe on a temp home, should get health_report over the daemon pipe",
  {
    skip:
      process.platform !== "win32"
        ? "Windows host only"
        : !engine &&
          !process.env.PU_REQUIRE_ENGINE &&
          "build pu-engine first (cargo build -p pu-engine) or set PU_ENGINE_PATH",
    timeout: 60000,
  },
  async () => {
    assert.ok(engine, "pu-engine.exe not found under target/");
    const home = mkdtempSync(path.join(os.tmpdir(), "pp-daemon-"));
    const socket = path.join(home, ".pu", "daemon.sock");
    mkdirSync(path.dirname(socket), { recursive: true });
    const env = { ...process.env, HOME: home, USERPROFILE: home };
    delete env.PU_HOME;
    const child = spawn(/** @type {string} */ (engine), ["--socket", socket], {
      env,
      stdio: "ignore",
      windowsHide: true,
    });
    const exited = new Promise((resolve) => child.once("exit", resolve));
    try {
      let health;
      const deadline = Date.now() + 30000;
      while (!health) {
        if (child.exitCode !== null)
          assert.fail(`pu-engine exited ${child.exitCode}`);
        try {
          health = await daemonRequest(
            socket,
            { type: "health" },
            { timeout: 2000 },
          );
        } catch (error) {
          if (Date.now() > deadline) throw error;
          await new Promise((r) => setTimeout(r, 200));
        }
      }
      assert.equal(health.type, "health_report");
      assert.ok(Number.isInteger(health.protocol_version));
      // The bridge connected by name, not by a socket file.
      assert.match(pipeNameForSocket(socket), /-daemon$/);
    } finally {
      child.kill();
      await exited;
      rmSync(home, { recursive: true, force: true, maxRetries: 5 });
    }
  },
);

test("Tailscale on Windows: fixed Program Files paths, never PATH, and no Unix operator hint", () => {
  assert.deepEqual(windowsTailscalePaths({ ProgramFiles: "D:\\Apps" }), [
    "D:\\Apps\\Tailscale\\tailscale.exe",
    "C:\\Program Files\\Tailscale\\tailscale.exe",
  ]);
  assert.deepEqual(
    windowsTailscalePaths({ ProgramFiles: "relative", PATH: "C:\\evil" }),
    ["C:\\Program Files\\Tailscale\\tailscale.exe"],
  );
  assert.deepEqual(tailscalePaths("win32", {}), windowsTailscalePaths({}));
  assert.ok(tailscalePaths("linux").every((p) => p.startsWith("/")));
  assert.deepEqual(serveCommands(8787, "win32"), [
    "tailscale serve --bg --tcp 8787 tcp://127.0.0.1:8787",
  ]);
  assert.equal(serveCommands(8787, "linux").length, 2);
});

test("headless Windows discovery binds only the address Tailscale and an interface agree on", async () => {
  const run = (output) => async () => output;
  const paths = [process.execPath];
  const tailscale = (address) => ({
    Tailscale: [
      { address: "fd7a:115c:a1e0::1", family: "IPv6", internal: false },
      { address, family: "IPv4", internal: false },
    ],
    "Wi-Fi": [{ address: "192.168.4.32", family: "IPv4", internal: false }],
  });
  assert.deepEqual(
    await discoverTailnet({
      paths: ["C:\\nonexistent\\tailscale.exe"],
      interfaces: tailscale("100.65.1.2"),
      run: run("100.65.1.2\r\n"),
    }),
    { mode: "unavailable", reason: "tailscale_missing" },
  );
  const agree = await discoverTailnet({
    paths,
    interfaces: tailscale("100.65.1.2"),
    run: run("100.65.1.2\r\n"),
  });
  assert.equal(agree.mode, "interface");
  assert.equal(agree.address, "100.65.1.2");
  assert.equal(
    (
      await discoverTailnet({
        paths,
        interfaces: tailscale("100.65.1.3"),
        run: run("100.65.1.2\r\n"),
      })
    ).reason,
    "tailnet_mismatch",
  );
});

test("without Tailscale the headless Windows bridge never listens and reports why", async () => {
  const pending = [];
  const binds = [];
  const remote = await startRemote({
    port: 8787,
    peerAllowed: (mode) => mode,
    discover: () =>
      discoverTailnet({
        paths: ["C:\\nonexistent\\tailscale.exe"],
        interfaces: {},
        run: async () => "100.65.1.2\n",
      }),
    listen: async (host) => {
      binds.push(host);
      return { shutdown: async () => {} };
    },
    setTimer: (fn, ms) => {
      const timer = { fn, ms };
      pending.push(timer);
      return timer;
    },
    clearTimer: (timer) => pending.splice(pending.indexOf(timer), 1),
  });
  assert.deepEqual(binds, []);
  assert.equal(remote.chatURL(), null);
  const status = remote.status();
  assert.equal(status.state, "unavailable");
  assert.equal(status.code, "tailscale_missing");
  assert.match(status.message, /not installed/);
  await remote.shutdown();
});

test("not writable by others: read grants to anyone pass, any write grant to another principal fails", () => {
  const acl = (aces) => ({ protected: false, aces });
  const ace = (sid, rights, flags = "") => ({ type: "A", flags, rights, sid });
  const profile = acl([
    ace(USER, "FA", "OICIID"),
    ace("S-1-5-18", "FA", "OICIID"),
    ace("S-1-5-32-544", "FA", "OICIID"),
    ace("BU", "0x1200a9", "OICIID"), // Users: read and execute
    ace("WD", "FR"),
    ace("CO", "FA", "OICIIO"), // inherit-only: never applies to the entry
  ]);
  assert.equal(writableByOthers(profile, USER), false);
  for (const rights of ["FA", "FW", "0x1301bf", "0x2", "WD", "FRFW", "zz"])
    assert.equal(
      writableByOthers(acl([ace("BU", rights)]), USER),
      true,
      rights,
    );
  assert.equal(grantsWrite("FRFX"), false);
});

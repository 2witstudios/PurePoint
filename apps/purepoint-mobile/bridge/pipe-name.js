import { createHash } from "node:crypto";
import { mkdirSync, realpathSync } from "node:fs";
import path from "node:path";

/**
 * JS port of `crates/pu-transport/src/naming.rs` (DEC-2, WINAPP-003): Windows
 * endpoints are `\\.\pipe\purepoint-<hash16>-<role>`, hashed over the
 * normalized socket path. Must stay byte-identical to the Rust implementation.
 */
export function normalizeText(text) {
  let s = String(text).replaceAll("/", "\\");
  if (s.length >= 2 && s[1] === ":") s = s[0].toUpperCase() + s.slice(1);
  while (s.endsWith("\\") && !(s.length === 3 && s[1] === ":"))
    s = s.slice(0, -1);
  return s;
}

/**
 * Create the parent directory, canonicalize it (junctions, `..`, 8.3 names,
 * on-disk casing; libuv strips `\\?\` like `dunce`), append the file name.
 */
export function normalizeSocketPath(socketPath) {
  const parent = path.win32.dirname(socketPath) || ".";
  const file = path.win32.basename(socketPath);
  if (!file) throw new Error("socket path has no file name");
  mkdirSync(parent, { recursive: true });
  return normalizeText(path.win32.join(realpathSync.native(parent), file));
}

/** First 8 bytes of SHA-256 over the UTF-8 string, lowercase hex. */
export function hash16(normalized) {
  return createHash("sha256")
    .update(normalized, "utf8")
    .digest()
    .subarray(0, 8)
    .toString("hex");
}

export function pipeName(normalized, role) {
  return `\\\\.\\pipe\\purepoint-${hash16(normalized)}-${role}`;
}

export function pipeNameForSocket(socketPath, role = "daemon") {
  return pipeName(normalizeSocketPath(socketPath), role);
}

/** The path `net.createConnection` should use: the pipe on Windows, else unchanged. */
export function daemonEndpoint(socketPath, platform = process.platform) {
  return platform === "win32" ? pipeNameForSocket(socketPath) : socketPath;
}

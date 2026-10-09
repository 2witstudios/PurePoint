import QRCode from "qrcode";
import { mkdir, writeFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { randomUUID } from "node:crypto";
import { pairingEndpoint } from "./endpoints.js";
export const pairingPagePath = path.join(
  os.homedir(),
  ".config/pi-mobile/pairing.html",
);

/** Encode only an ephemeral enrollment; durable shared secrets are never QR authorization. */
export function pairingPayload(payload) {
  const code = typeof payload === "string" ? JSON.parse(payload) : payload;
  pairingEndpoint(code.endpoint);
  if (code.type !== "pi-mobile-pairing" || code.version !== 2 || typeof code.hostId !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(code.enrollmentToken) || !/^[a-f0-9]{64}$/.test(code.certificateSHA256) || !Number.isSafeInteger(code.expiresAt) || code.expiresAt <= Date.now() || code.expiresAt > Date.now() + 300000 || Object.hasOwn(code, "secret"))
    throw new Error("QR pairing requires a current one-time enrollment and pinned host identity.");
  return JSON.stringify({ type: code.type, version: code.version, endpoint: code.endpoint, hostId: code.hostId, certificateSHA256: code.certificateSHA256, enrollmentToken: code.enrollmentToken, expiresAt: code.expiresAt });
}

export async function writePairingPage(file, payload) {
  const svg = await QRCode.toString(pairingPayload(payload), {
    type: "svg", errorCorrectionLevel: "M", margin: 4,
  });
  const html = `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Pair your iPhone · Pi</title>
<style>body{margin:0;background:#f5f5f7;color:#18181b;font-family:system-ui;display:grid;place-items:center;min-height:100vh}main{max-width:440px;margin:32px;padding:32px;border-radius:24px;background:white;box-shadow:0 12px 60px #0000000a}h1{font-size:32px;letter-spacing:-1px}p{line-height:1.6;color:#52525b}svg{width:100%;display:block}small{display:block;color:#71717a;line-height:1.5}</style>
<main><h1>Your Mac. Your Pi.</h1><p>On your iPhone, open Pi → Connection → <strong>Scan Mac QR code</strong>.</p>${svg}<p>Keep Tailscale connected on both devices.</p><small>This private QR enrolls one phone and expires shortly. Create a new code in PurePoint if it expires. No information is uploaded.</small></main></html>`;
  await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = file + "." + randomUUID() + ".tmp";
  try {
    await writeFile(temporary, html, { mode: 0o600, flag: "wx" });
    await rename(temporary, file);
  } finally {
    await rm(temporary, { force: true });
  }
}

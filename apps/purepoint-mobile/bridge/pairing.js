import QRCode from "qrcode";
import { mkdir, writeFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { randomUUID } from "node:crypto";
import { allowedHost } from "./network.js";
export const pairingPagePath = path.join(
  os.homedir(),
  ".config/pi-mobile/pairing.html",
);

export function pairingPayload(endpoint, secret) {
  const url = new URL(endpoint);
  const host = url.hostname.replace(/^\[|\]$/g, "");
  if (
    !["ws:", "wss:"].includes(url.protocol) ||
    !allowedHost(host) ||
    url.pathname !== "/v1" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  )
    throw new Error(
      "Pairing requires a tailnet or loopback bridge endpoint ending in /v1.",
    );
  if (
    secret.length < 32 ||
    Buffer.byteLength(secret) > 1024 ||
    /[\r\n]/.test(secret)
  )
    throw new Error(
      "QR pairing requires a secret of 32–1024 bytes without newlines.",
    );
  return JSON.stringify({
    type: "pi-mobile-pairing",
    version: 1,
    endpoint: url.href,
    secret,
  });
}

export async function writePairingPage(file, endpoint, secret) {
  const svg = await QRCode.toString(pairingPayload(endpoint, secret), {
    type: "svg",
    errorCorrectionLevel: "M",
    margin: 4,
  });
  const html = `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Pair your iPhone · Pi</title>
<style>body{margin:0;background:#f5f5f7;color:#18181b;font-family:system-ui;display:grid;place-items:center;min-height:100vh}main{max-width:440px;margin:32px;padding:32px;border-radius:24px;background:white;box-shadow:0 12px 60px #0000000a}h1{font-size:32px;letter-spacing:-1px}p{line-height:1.6;color:#52525b}svg{width:100%;display:block}small{display:block;color:#71717a;line-height:1.5}</style>
<main><h1>Your Mac. Your Pi.</h1><p>On your iPhone, open Pi → Connection → <strong>Scan Mac QR code</strong>.</p>${svg}<p>Keep Tailscale connected on both devices.</p><small>This QR contains your pairing secret. Keep it private and close this page when you’re done. No information is uploaded.</small></main></html>`;
  await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = file + "." + randomUUID() + ".tmp";
  try {
    await writeFile(temporary, html, { mode: 0o600, flag: "wx" });
    await rename(temporary, file);
  } finally {
    await rm(temporary, { force: true });
  }
}

import { ensureToken } from "./setup.js";
import { writePairingPage, pairingPagePath } from "./pairing.js";

async function main() {
  const host = process.env.PI_MOBILE_HOST;
  if (!host) throw new Error("Set PI_MOBILE_HOST to your Mac’s Tailscale IP.");
  const port = Number(process.env.PI_MOBILE_PORT ?? 8787);
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("Invalid bridge port.");
  const token = await ensureToken(process.env.PI_MOBILE_TOKEN_FILE);
  const protocol =
    process.env.PI_MOBILE_TLS_CERT && process.env.PI_MOBILE_TLS_KEY
      ? "wss"
      : "ws";
  const endpoint = `${protocol}://${host.includes(":") ? "[" + host + "]" : host}:${port}/v1`;
  const file = pairingPagePath;
  await writePairingPage(file, endpoint, token);
  console.log(
    `Private pairing QR saved to ${file}. Open this file locally, then scan it in Pi → Connection. The bridge must be running.`,
  );
}
main().catch(() => {
  console.error(
    "Could not create pairing QR. Check PI_MOBILE_HOST, PI_MOBILE_PORT and your private PI_MOBILE_TOKEN_FILE (32–1024 bytes, no newlines, chmod 600).",
  );
  process.exitCode = 1;
});

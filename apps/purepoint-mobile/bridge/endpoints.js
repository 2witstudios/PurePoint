import { isIP } from "node:net";
export function allowedHost(host) {
  if (host === "127.0.0.1" || host === "::1") return true;
  if (isIP(host) === 4) {
    const p = host.split(".").map(Number);
    return p[0] === 100 && p[1] >= 64 && p[1] <= 127;
  }
  return isIP(host) === 6 && host.toLowerCase().startsWith("fd7a:115c:a1e0:");
}
export function pairingEndpoint(endpoint) {
  const url = new URL(endpoint);
  if (
    url.protocol !== "wss:" ||
    !allowedHost(url.hostname.replace(/^\[|\]$/g, "")) ||
    url.pathname !== "/v1" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  )
    throw new Error(
      "Enrollment requires an explicit tailnet or loopback WSS endpoint ending in /v1.",
    );
  return url;
}

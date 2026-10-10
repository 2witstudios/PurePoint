import { createServer } from "node:http";
import { timingSafeEqual } from "node:crypto";
/** Safe public errors must be constructed explicitly; raw SDK errors are never forwarded. */
export class AdminError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}
function matches(actual, expected) {
  const a = Buffer.from(actual ?? "");
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}
/** Owner-only HTTP authority on a distinct loopback ephemeral listener. */
export async function serveAdmin({ token, dispatch }) {
  const server = createServer(async (req, res) => {
    const send = (status, value) => {
      res.writeHead(status, {
        "content-type": "application/json",
        "cache-control": "no-store",
        connection: "close",
      });
      res.end(JSON.stringify(value));
    };
    const fail = (status, code, message) =>
      send(status, { ok: false, error: { code, message } });
    if (req.headers.origin !== undefined)
      return fail(403, "origin", "Browser requests are not allowed.");
    if (req.method !== "POST") return fail(405, "method", "Use POST.");
    if (req.url !== "/admin/v1")
      return fail(404, "route", "Unknown administrative route.");
    if (!matches(req.headers.authorization, `Bearer ${token}`))
      return fail(401, "authorization", "Owner authentication required.");
    if (req.headers["content-type"] !== "application/json")
      return fail(415, "content_type", "Use JSON.");
    const size = Number(req.headers["content-length"]);
    if (size > 65536)
      return fail(413, "size", "Administrative request too large.");
    try {
      let bytes = 0;
      const chunks = [];
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 65536)
          return fail(413, "size", "Administrative request too large.");
        chunks.push(chunk);
      }
      let request;
      try {
        request = JSON.parse(Buffer.concat(chunks).toString());
      } catch {
        return fail(400, "json", "Invalid JSON.");
      }
      if (
        !request ||
        typeof request.operation !== "string" ||
        Array.isArray(request)
      )
        return fail(400, "operation", "Administrative operation required.");
      send(200, { ok: true, result: await dispatch(request) });
    } catch (e) {
      fail(
        400,
        e instanceof AdminError ? e.code : "operation_failed",
        e instanceof AdminError
          ? e.message
          : "Operation failed. Inspect setup status and retry.",
      );
    }
  });
  server.requestTimeout = 10000;
  server.headersTimeout = 10000;
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve(null);
    });
  });
  return {
    url: `http://127.0.0.1:${/** @type {import("node:net").AddressInfo} */ (server.address()).port}/admin/v1`,
    shutdown: () =>
      new Promise((resolve) => {
        server.close(() => resolve(null));
        server.closeAllConnections();
      }),
  };
}

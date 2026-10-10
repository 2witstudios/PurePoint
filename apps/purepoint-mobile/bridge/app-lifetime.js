/** Only the app-owned launch uses stdin as a private lifetime channel. */
export function watchAppLifetime(env, onLost, input = process.stdin) {
  if (env.POINT_GUARD_APP_LIFETIME === undefined) return () => {};
  if (env.POINT_GUARD_APP_LIFETIME !== "stdin")
    throw new Error("Invalid Point Guard app lifetime channel.");
  let attached = true;
  const dispose = () => {
    if (!attached) return;
    attached = false;
    input.removeListener("end", lost);
    input.removeListener("close", lost);
    input.removeListener("error", lost);
    input.pause();
  };
  const lost = () => {
    if (!attached) return;
    dispose();
    onLost();
  };
  input.once("end", lost);
  input.once("close", lost);
  input.once("error", lost);
  if (input.destroyed || input.readableEnded) queueMicrotask(lost);
  else input.resume();
  return dispose;
}

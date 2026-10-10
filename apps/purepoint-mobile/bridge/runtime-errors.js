/** Classify into static safe messages; never return arbitrary provider/SDK error text. */
export function startupFailure(error, stage, instanceId, pid = process.pid) {
  const message =
    typeof error?.message === "string" ? error.message.toLowerCase() : "";
  let code = "startup_failed",
    recovery =
      "Check the complete app runtime and private state, then retry. No uncertain prompt will be replayed.";
  if (stage === "canceled") {
    code = "startup_canceled";
    recovery =
      "Point Guard startup was canceled. Its owned child and startup lock have been released; retry when ready.";
  } else if (stage === "lock" && (message.includes("startup lock") || message.includes("already running"))) {
    code = "startup_collision";
    recovery =
      "Quit the app that owns Point Guard. If it crashed, use deliberate recovery for its private startup lock; no other process will be adopted or stopped.";
  } else if (stage === "lock") {
    code = message.includes("private") || message.includes("owned") ? "private_state" : "state_corrupt";
    recovery = code === "private_state"
      ? "Restore owner-only permissions for Point Guard state before retrying."
      : "Point Guard state is corrupt or uses an unsupported schema. Restore a known private backup before retrying.";
  } else if (stage === "runtime") {
    code = "missing_runtime";
    recovery =
      "Reinstall the complete PurePoint app for this Mac architecture. Bundled Node, Pi, pu and support files are required.";
  } else if (stage === "cwd") {
    code = "invalid_cwd";
    recovery = "Choose an existing working folder in Point Guard setup.";
  } else if (stage === "credentials") {
    code = "credential_state";
    recovery =
      "Native Pi credentials are corrupt or not private. Restore owner-only permissions or a known private backup before retrying.";
  } else if (stage === "trust") {
    code = message.includes("expired")
      ? "identity_expired"
      : message.includes("private") || message.includes("owner")
        ? "private_state"
        : "identity_corrupt";
    recovery =
      code === "identity_expired"
        ? "Host TLS identity expired. Restore a valid original identity or deliberately reset trust and re-pair every phone."
        : code === "private_state"
          ? "Restore private owner-only permissions for Point Guard trust state before retrying."
          : "Host identity is missing, corrupt or mismatched. Restore the original private identity or deliberately reset trust and re-pair every phone.";
  } else if (error?.code === "EADDRINUSE") {
    code = "listener_collision";
    recovery =
      "The configured phone port is already occupied. Choose another port or quit its owning app; Point Guard will never adopt or stop that listener.";
  } else if (stage === "pi") {
    code = "native_session";
    recovery =
      "The native Pi session or packaged adapter could not load. Inspect native session state, select a valid conversation/provider, or reinstall the complete app. Nothing will be replayed.";
  }
  return {
    schemaVersion: 1,
    instanceId,
    pid,
    code,
    message: "Point Guard could not start its packaged service.",
    recovery,
  };
}

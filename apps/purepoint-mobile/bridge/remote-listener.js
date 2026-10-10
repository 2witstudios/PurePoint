/** Only an unavailable configured interface may degrade to independent native service. */
export async function optionalRemoteListener(start) {
  try {
    return { server: await start() };
  } catch (error) {
    if (!["EADDRNOTAVAIL", "ENETUNREACH", "EHOSTUNREACH"].includes(error?.code))
      throw error;
    return {
      remoteRecovery: {
        code: "remote_unavailable",
        message: "The configured phone interface is unavailable.",
        recovery:
          "Restore the configured private network interface, then restart Point Guard when idle. Native chat remains available; phone pairing is unavailable until the actual pinned listener starts.",
      },
    };
  }
}

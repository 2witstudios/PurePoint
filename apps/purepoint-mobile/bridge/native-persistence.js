const installed = new WeakSet();
/** Pi 1.1.0 defers setup-only trees. Managed selection must survive restart before first prompt. */
export function installManagedPersistence(SessionManager) {
  if (installed.has(SessionManager)) return;
  const p = SessionManager.prototype;
  if (
    typeof p._persist !== "function" ||
    typeof p._hasConversation !== "function"
  )
    throw new Error("Pinned native session persistence API changed.");
  const persist = p._persist;
  p._hasConversation = function () {
    return true;
  };
  p._persist = function (entry) {
    // Native synchronous writer and tree semantics remain authoritative; newly created files are private.
    const previous = process.umask(0o077);
    try {
      return persist.call(this, entry);
    } finally {
      process.umask(previous);
    }
  };
  installed.add(SessionManager);
}

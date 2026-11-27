// Port of apps/purepoint-macos/purepoint-macos/State/CommandPaletteOpenCoordinator.swift (WIN-6.6).

/** Coalesces palette open requests across all entry points while their data loads. */
export class CommandPaletteOpenCoordinator {
  private isOpening = false;

  /**
   * Runs `open` unless an earlier request is still running; returns its completion, or null
   * when the request was coalesced into the one in flight.
   */
  request(open: () => Promise<void> | void): Promise<void> | null {
    if (this.isOpening) return null;
    // Set this before scheduling so same-turn requests also coalesce.
    this.isOpening = true;
    return (async () => {
      try {
        // Like the Mac's `Task`, the open runs after the requesting turn.
        await Promise.resolve();
        await open();
      } finally {
        this.isOpening = false;
      }
    })();
  }
}

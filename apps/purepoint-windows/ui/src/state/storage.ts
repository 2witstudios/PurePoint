/** `localStorage` when the environment has one and allows it; null otherwise (kept in memory). */
export function browserStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/**
 * "Latest request wins" guard for async loads.
 *
 * Each `begin(key)` starts a new request for that key and returns `isCurrent()`, which stays true
 * only until another request for the same key begins or the key is invalidated. A caller checks
 * it after every await and drops a response that is no longer current, so a slow answer to an
 * older request can never overwrite the state set by a newer one.
 *
 *   const isCurrent = guard.begin("openWo");
 *   const resp = await getWo(id);
 *   if (!isCurrent()) return;
 *   set({ activeWo: resp });
 */
export interface LatestRequestGuard {
  /** Starts a request for `key`; the returned function tells whether it is still the latest. */
  begin(key?: string): () => boolean;
  /** Makes every request already started for `key` stale (e.g. the view was closed). */
  invalidate(key?: string): void;
}

const DEFAULT_KEY = "default";

export function createLatestRequestGuard(): LatestRequestGuard {
  const latest = new Map<string, number>();
  const next = (key: string) => {
    const n = (latest.get(key) ?? 0) + 1;
    latest.set(key, n);
    return n;
  };
  return {
    begin(key = DEFAULT_KEY) {
      const mine = next(key);
      return () => latest.get(key) === mine;
    },
    invalidate(key = DEFAULT_KEY) {
      next(key);
    },
  };
}

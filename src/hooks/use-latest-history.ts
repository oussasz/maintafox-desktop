/**
 * Loads the most recent page of a record's history (audit / change events) and pages backwards.
 *
 * The backend returns the `limit` latest events in chronological order. The hook asks for one
 * more than it shows: when that extra event comes back, older events exist and the timeline
 * says so (`hasOlder`) instead of capping silently. `loadOlder` widens the page up to `maxLimit`
 * (the backend's own maximum); past it `canLoadOlder` is false and the notice stays.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface LatestHistoryState<T> {
  items: T[];
  loading: boolean;
  error: unknown;
  /** Older events exist beyond what is shown. */
  hasOlder: boolean;
  /** `loadOlder` can still widen the page. */
  canLoadOlder: boolean;
  loadingOlder: boolean;
  loadOlder: () => void;
  reload: () => void;
}

export function useLatestHistory<T>(
  fetchLatest: (limit: number) => Promise<T[]>,
  options: { pageSize: number; maxLimit: number },
): LatestHistoryState<T> {
  const { pageSize, maxLimit } = options;
  const initialLimit = Math.min(pageSize, maxLimit);
  // The widened page belongs to one record: a different fetch function starts from page one.
  const [page, setPage] = useState({ fetcher: fetchLatest, limit: initialLimit });
  const limit = page.fetcher === fetchLatest ? page.limit : initialLimit;
  const [items, setItems] = useState<T[]>([]);
  const [hasOlder, setHasOlder] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [nonce, setNonce] = useState(0);
  /** Fetch function whose first page is on screen (then a new limit is "load older"). */
  const shownFor = useRef<unknown>(null);

  useEffect(() => {
    let cancelled = false;
    const probe = Math.min(limit + 1, maxLimit);
    const older = shownFor.current === fetchLatest;
    if (older) setLoadingOlder(true);
    else setLoading(true);
    setError(null);
    fetchLatest(probe)
      .then((rows) => {
        if (cancelled) return;
        const more = rows.length > limit || (limit >= maxLimit && rows.length >= maxLimit);
        setHasOlder(more);
        setItems(rows.length > limit ? rows.slice(rows.length - limit) : rows);
        shownFor.current = fetchLatest;
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err);
        setItems([]);
        setHasOlder(false);
        shownFor.current = null;
      })
      .finally(() => {
        if (cancelled) return;
        setLoading(false);
        setLoadingOlder(false);
      });
    return () => {
      cancelled = true;
    };
  }, [fetchLatest, limit, maxLimit, nonce]);

  const canLoadOlder = hasOlder && limit < maxLimit;

  const loadOlder = useCallback(() => {
    if (!canLoadOlder) return;
    setPage({ fetcher: fetchLatest, limit: Math.min(limit + pageSize, maxLimit) });
  }, [canLoadOlder, fetchLatest, limit, pageSize, maxLimit]);

  const reload = useCallback(() => {
    shownFor.current = null;
    setNonce((n) => n + 1);
  }, []);

  return { items, loading, error, hasOlder, canLoadOlder, loadingOlder, loadOlder, reload };
}

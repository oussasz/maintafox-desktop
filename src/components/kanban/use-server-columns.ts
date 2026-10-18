/**
 * Board lanes read from the server, one paged request per lane (the list's own paged call with
 * the lane's statuses): each lane shows its real total, and "load more" asks the server for the
 * lane's next page instead of revealing only cards the list happened to have loaded.
 */
import { useCallback, useEffect, useRef, useState } from "react";

export interface ServerLaneDef {
  id: string;
  /** Statuses read for this lane; empty = the lane has nothing under the current filter. */
  statuses: readonly string[];
}

export interface ServerLanePage<T> {
  items: T[];
  total: number;
}

interface LaneState<T> {
  items: T[];
  total: number;
  loadingMore: boolean;
}

export interface ServerColumns<T> {
  /** Cards of every lane, in lane order. */
  items: T[];
  totals: Record<string, number>;
  loadingMore: Record<string, boolean>;
  loading: boolean;
  error: unknown;
  loadMore: (laneId: string) => void;
  reload: () => void;
}

export function useServerColumns<T extends { id: number }>({
  lanes,
  fetchPage,
  pageSize,
  queryKey,
}: {
  lanes: readonly ServerLaneDef[];
  fetchPage: (
    statuses: readonly string[],
    offset: number,
    limit: number,
  ) => Promise<ServerLanePage<T>>;
  pageSize: number;
  /** Changes whenever the filter behind the lanes changes (lanes are then read again). */
  queryKey: string;
}): ServerColumns<T> {
  const [state, setState] = useState<Record<string, LaneState<T>>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [reloadTick, setReloadTick] = useState(0);
  const generation = useRef(0);
  const lanesRef = useRef(lanes);
  lanesRef.current = lanes;
  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;

  useEffect(() => {
    const gen = ++generation.current;
    setLoading(true);
    setError(null);
    void Promise.all(
      lanesRef.current.map(async (lane) =>
        lane.statuses.length === 0
          ? ([lane.id, { items: [] as T[], total: 0 }] as const)
          : ([lane.id, await fetchRef.current(lane.statuses, 0, pageSize)] as const),
      ),
    )
      .then((pages) => {
        if (gen !== generation.current) return;
        const next: Record<string, LaneState<T>> = {};
        for (const [id, page] of pages) {
          next[id] = { items: page.items, total: page.total, loadingMore: false };
        }
        setState(next);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (gen !== generation.current) return;
        setError(err);
        setLoading(false);
      });
  }, [queryKey, pageSize, reloadTick]);

  const loadMore = useCallback(
    (laneId: string) => {
      const lane = lanesRef.current.find((l) => l.id === laneId);
      const current = state[laneId];
      if (!lane || !current || current.loadingMore || current.items.length >= current.total) return;
      const gen = generation.current;
      setState((s) => ({ ...s, [laneId]: { ...current, loadingMore: true } }));
      fetchRef
        .current(lane.statuses, current.items.length, pageSize)
        .then((page) => {
          if (gen !== generation.current) return;
          setState((s) => {
            const lanePrev = s[laneId] ?? current;
            const known = new Set(lanePrev.items.map((i) => i.id));
            return {
              ...s,
              [laneId]: {
                items: [...lanePrev.items, ...page.items.filter((i) => !known.has(i.id))],
                total: page.total,
                loadingMore: false,
              },
            };
          });
        })
        .catch(() => {
          // The global IPC toast reports the failure; the loaded cards stay usable.
          if (gen !== generation.current) return;
          setState((s) => ({ ...s, [laneId]: { ...(s[laneId] ?? current), loadingMore: false } }));
        });
    },
    [state, pageSize],
  );

  const items: T[] = [];
  const totals: Record<string, number> = {};
  const loadingMore: Record<string, boolean> = {};
  for (const lane of lanes) {
    const s = state[lane.id];
    if (s) items.push(...s.items);
    totals[lane.id] = s?.total ?? 0;
    loadingMore[lane.id] = s?.loadingMore ?? false;
  }

  return {
    items,
    totals,
    loadingMore,
    loading,
    error,
    loadMore,
    reload: () => setReloadTick((n) => n + 1),
  };
}

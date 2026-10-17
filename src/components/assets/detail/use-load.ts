/**
 * Loads one resource for an asset detail tab: data, raw error (for ErrorState), loading flag and
 * a reload. A late answer for a previous key is ignored.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface LoadState<T> {
  data: T | null;
  error: unknown;
  loading: boolean;
  reload: () => Promise<void>;
  setData: (next: T | null) => void;
}

export function useLoad<T>(load: (() => Promise<T>) | null, key: string): LoadState<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(load !== null);
  const seq = useRef(0);
  const loadRef = useRef(load);
  loadRef.current = load;

  const reload = useCallback(async () => {
    const fn = loadRef.current;
    if (!fn) return;
    const mine = ++seq.current;
    setLoading(true);
    setError(null);
    try {
      const next = await fn();
      if (mine === seq.current) setData(next);
    } catch (err) {
      if (mine === seq.current) setError(err);
    } finally {
      if (mine === seq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    setData(null);
    void reload();
  }, [key, reload]);

  return { data, error, loading, reload, setData };
}

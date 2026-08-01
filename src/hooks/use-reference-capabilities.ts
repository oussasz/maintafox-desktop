import { useEffect, useState } from "react";

import { getReferenceGovernanceCapabilities } from "@/services/reference-service";
import type { ReferenceGovernanceCapabilities } from "@shared/ipc-types";

/**
 * Loads governance capabilities for a real reference domain (+ optional set).
 * Synthetic domains (negative ids) return null — callers must not mutate them via this path.
 */
export function useReferenceCapabilities(
  domainId: number | null | undefined,
  setId: number | null | undefined,
  /** When set status changes (e.g. after publish), refetch capabilities. */
  setStatus?: string | null,
): {
  caps: ReferenceGovernanceCapabilities | null;
  loading: boolean;
  error: string | null;
  reload: () => void;
} {
  const [caps, setCaps] = useState<ReferenceGovernanceCapabilities | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (domainId == null || domainId < 0) {
      setCaps(null);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);

    getReferenceGovernanceCapabilities(domainId, setId ?? null)
      .then((c) => {
        if (!cancelled) setCaps(c);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setCaps(null);
          setError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [domainId, setId, setStatus, tick]);

  return {
    caps,
    loading,
    error,
    reload: () => setTick((t) => t + 1),
  };
}

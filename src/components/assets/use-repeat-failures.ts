/**
 * The equipment flagged "pannes répétitives" now (rule from Paramètres > Fiabilité), read once
 * for a list. A failed read is returned as an error, never as "no flag".
 */

import { useEffect, useMemo, useState } from "react";

import { listRepeatFailureAssets } from "@/services/asset-follow-up-service";
import type { RepeatFailureAsset, RepeatFailureOverview } from "@shared/ipc-types";

export interface RepeatFailureFlags {
  overview: RepeatFailureOverview | null;
  byAsset: Map<number, RepeatFailureAsset>;
  error: unknown;
}

export function useRepeatFailureAssets(refreshKey: unknown = null): RepeatFailureFlags {
  const [overview, setOverview] = useState<RepeatFailureOverview | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let cancelled = false;
    listRepeatFailureAssets()
      .then((o) => {
        if (cancelled) return;
        setOverview(o);
        setError(null);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);
  const byAsset = useMemo(
    () => new Map((overview?.assets ?? []).map((a) => [a.asset_id, a])),
    [overview],
  );
  return { overview, byAsset, error };
}

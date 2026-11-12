/**
 * Failure figures of one asset from the reliability (RAMS) calculation, over the RAMS default
 * period: the single source of the failure count and MTBF shown on the asset record, so the record
 * and the RAMS screen never disagree. Only asked of a user who may open RAMS (`ram.view`).
 */

import { useMemo } from "react";

import { type LoadState, useLoad } from "@/components/assets/detail/use-load";
import { RAMS_DEFAULT_MONTHS, periodFromMonths } from "@/components/reliability/study/rams-ui";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type AssetReliabilityFigures,
  getReliabilityAssetFigures,
} from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

export interface AssetRamFigures extends LoadState<AssetReliabilityFigures> {
  /** The user may read RAMS figures; without it the record keeps its own work-order counts. */
  allowed: boolean;
  /** Length of the window the figures cover, in months. */
  months: number;
}

export function useAssetRamFigures(assetId: number): AssetRamFigures {
  const { can } = usePermissions();
  const allowed = can(P.RAM_VIEW);
  const months = RAMS_DEFAULT_MONTHS;
  const load = useMemo(
    () =>
      allowed
        ? () => {
            const period = periodFromMonths(months);
            return getReliabilityAssetFigures(assetId, period.start, period.end);
          }
        : null,
    [allowed, assetId, months],
  );
  const state = useLoad(load, `${assetId}:${allowed}`);
  return { ...state, allowed, months };
}

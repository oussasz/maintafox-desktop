/**
 * Quantities entered in another unit than the article's stock unit (receipt in bags, issue in
 * grams…). The factor comes from the server (`preview_inventory_stock_quantity`, the same rule as
 * the posting: the article's own conversion, then the plant unit's factor, then a general
 * conversion between standard units); the stock quantity is rounded like the server does.
 */

import { useEffect, useState } from "react";

import { previewInventoryStockQuantity } from "@/services/unit-conversion-service";

/** Decimals kept on a converted stock quantity (backend `STOCK_QUANTITY_DECIMALS`). */
export const STOCK_QUANTITY_DECIMALS = 6;

/** Rounds a converted stock quantity half away from zero, like the backend. */
export function roundStockQuantity(quantity: number): number {
  const scale = 10 ** STOCK_QUANTITY_DECIMALS;
  return (Math.sign(quantity) * Math.round(Math.abs(quantity) * scale)) / scale;
}

/** Stock quantity of `entered` units at `factor` stock units per entered unit. */
export function toStockQuantity(entered: number, factor: number): number {
  return roundStockQuantity(entered * factor);
}

export interface EntryFactor {
  /** Stock units per entered unit; 1 in the stock unit; null while unknown or refused. */
  factor: number | null;
  /** Refusal of the server (no conversion, other dimension, unit to review…). */
  error: unknown;
  loading: boolean;
}

/**
 * Factor from `unitId` to the stock unit of the article. `unitId` null or equal to the stock unit:
 * 1 without asking the server.
 */
export function useEntryFactor(
  articleId: number | null,
  unitId: number | null,
  stockUnitId: number | null,
): EntryFactor {
  const same = unitId == null || unitId === stockUnitId;
  const [state, setState] = useState<EntryFactor>({ factor: 1, error: null, loading: false });
  useEffect(() => {
    if (same || articleId == null) {
      setState({ factor: 1, error: null, loading: false });
      return;
    }
    let cancelled = false;
    setState({ factor: null, error: null, loading: true });
    previewInventoryStockQuantity(articleId, unitId, 1)
      .then((preview) => {
        if (!cancelled) setState({ factor: preview.factor, error: null, loading: false });
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ factor: null, error, loading: false });
      });
    return () => {
      cancelled = true;
    };
  }, [articleId, unitId, same]);
  return state;
}

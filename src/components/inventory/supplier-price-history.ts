/**
 * Price history per supplier for one article (UX L3-13), built from the article's purchase
 * order lines: last and previous price, change between them, lowest and highest price paid.
 */

export interface PricedPurchase {
  supplier_id: number | null;
  supplier_name: string | null;
  unit_price: number | null;
  ordered_at: string | null;
  purchase_order_id: number;
}

export interface SupplierPriceSummary {
  supplierId: number | null;
  supplierName: string | null;
  purchases: number;
  lastPrice: number;
  lastOrderedAt: string | null;
  previousPrice: number | null;
  /** Change from the previous to the last price, in percent; `null` without a previous price. */
  changePct: number | null;
  minPrice: number;
  maxPrice: number;
}

function newestFirst(a: PricedPurchase, b: PricedPurchase): number {
  const da = a.ordered_at ?? "";
  const db = b.ordered_at ?? "";
  if (da !== db) return da < db ? 1 : -1;
  return b.purchase_order_id - a.purchase_order_id;
}

/** One summary per supplier that has at least one priced line, most recent purchase first. */
export function summarizeSupplierPrices(rows: readonly PricedPurchase[]): SupplierPriceSummary[] {
  const groups = new Map<string, PricedPurchase[]>();
  for (const row of rows) {
    if (row.unit_price == null) continue;
    const key = row.supplier_id == null ? "none" : String(row.supplier_id);
    const list = groups.get(key) ?? [];
    list.push(row);
    groups.set(key, list);
  }
  const summaries: SupplierPriceSummary[] = [];
  for (const list of groups.values()) {
    const sorted = [...list].sort(newestFirst);
    const [last, prev = null] = sorted;
    if (!last) continue;
    const prices = sorted.map((r) => r.unit_price as number);
    const lastPrice = last.unit_price as number;
    const previousPrice = prev ? (prev.unit_price as number) : null;
    summaries.push({
      supplierId: last.supplier_id,
      supplierName: last.supplier_name,
      purchases: sorted.length,
      lastPrice,
      lastOrderedAt: last.ordered_at,
      previousPrice,
      changePct:
        previousPrice != null && previousPrice !== 0
          ? ((lastPrice - previousPrice) / previousPrice) * 100
          : null,
      minPrice: Math.min(...prices),
      maxPrice: Math.max(...prices),
    });
  }
  return summaries.sort((a, b) => {
    const da = a.lastOrderedAt ?? "";
    const db = b.lastOrderedAt ?? "";
    return da === db ? 0 : da < db ? 1 : -1;
  });
}

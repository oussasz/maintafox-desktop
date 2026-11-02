/**
 * Side-by-side comparison of two budget versions: lines are grouped by cost centre and
 * category (all months added up) so the comparison stays readable.
 */

import type { BudgetLine } from "@shared/ipc-types";

export interface BudgetCompareRow {
  /** Unique per cost centre and category (two cost centres may share a code by mistake). */
  key: string;
  costCenterCode: string;
  costCenterName: string;
  bucket: string;
  left: number;
  right: number;
  /** right − left: what version B adds to (or removes from) version A. */
  delta: number;
}

/** Rounds to 2 decimals so sums of floats compare cleanly with 0. */
export function roundCents(value: number): number {
  return Math.round((value + Number.EPSILON) * 100) / 100;
}

export function compareBudgetLines(left: BudgetLine[], right: BudgetLine[]): BudgetCompareRow[] {
  const rows = new Map<string, BudgetCompareRow>();
  const touch = (l: BudgetLine): BudgetCompareRow => {
    const key = `${l.cost_center_id}|${l.budget_bucket}`;
    let row = rows.get(key);
    if (!row) {
      row = {
        key,
        costCenterCode: l.cost_center_code,
        costCenterName: l.cost_center_name,
        bucket: l.budget_bucket,
        left: 0,
        right: 0,
        delta: 0,
      };
      rows.set(key, row);
    }
    return row;
  };
  for (const l of left) touch(l).left += l.planned_amount;
  for (const l of right) touch(l).right += l.planned_amount;
  const out = [...rows.values()];
  for (const r of out) {
    r.left = roundCents(r.left);
    r.right = roundCents(r.right);
    r.delta = roundCents(r.right - r.left);
  }
  return out.sort(
    (a, b) => a.costCenterCode.localeCompare(b.costCenterCode) || a.bucket.localeCompare(b.bucket),
  );
}

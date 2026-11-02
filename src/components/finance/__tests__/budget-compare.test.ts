import { describe, expect, it } from "vitest";

import { compareBudgetLines } from "@/components/finance/budget-compare";
import type { BudgetLine } from "@shared/ipc-types";

function line(cc: number, bucket: string, month: number | null, amount: number): BudgetLine {
  return {
    cost_center_id: cc,
    cost_center_code: `CC${cc}`,
    cost_center_name: `Centre ${cc}`,
    budget_bucket: bucket,
    period_month: month,
    planned_amount: amount,
  } as BudgetLine;
}

describe("compareBudgetLines", () => {
  it("adds months up per cost centre and category and computes the gap", () => {
    const rows = compareBudgetLines(
      [line(1, "labor", 1, 100), line(1, "labor", 2, 100), line(2, "parts", null, 50)],
      [line(1, "labor", 1, 150), line(1, "labor", 2, 150), line(3, "contract", null, 70)],
    );
    expect(rows).toEqual([
      expect.objectContaining({ costCenterCode: "CC1", left: 200, right: 300, delta: 100 }),
      expect.objectContaining({ costCenterCode: "CC2", left: 50, right: 0, delta: -50 }),
      expect.objectContaining({ costCenterCode: "CC3", left: 0, right: 70, delta: 70 }),
    ]);
  });
});

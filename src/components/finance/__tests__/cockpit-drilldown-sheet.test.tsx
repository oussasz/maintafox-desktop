/**
 * Cockpit drill-down: a "load more" page that arrives after the user opened another cost centre
 * belongs to the first one. It must not be added to the second one's lines or replace its totals,
 * and the second one's "load more" must not stay stuck on "loading".
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BudgetCockpitDrilldown, BudgetCockpitRow } from "@/services/finance-cost-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "count" in opts ? `${key}:${String(opts["count"])}` : key,
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({ getBudgetCockpitDrilldown: vi.fn() }));
vi.mock("@/services/finance-cost-service", () => svc);
vi.mock("@/components/finance/use-finance-catalogs", () => ({
  useFinanceCatalogs: () => ({
    categoryLabel: (code: string) => code,
    periodLabel: (month: number) => `P${month}`,
  }),
}));

import { CockpitDrilldownSheet } from "../CockpitDrilldownSheet";

function row(costCenterId: number, code: string): BudgetCockpitRow {
  return {
    cost_center_id: costCenterId,
    cost_center_code: code,
    bucket: "services",
    budget: 1000,
    actual: 100,
    committed: 0,
  } as unknown as BudgetCockpitRow;
}

function line(id: number, equipment: string) {
  return {
    actual_id: id,
    period_month: 10,
    bucket: "services",
    amount: 10,
    posting_status: "posted",
    source_type: "manual",
    source_id: `m:${id}`,
    rate_card_lane: null,
    work_order_id: null,
    work_order_code: null,
    work_order_title: null,
    equipment_code: equipment,
    event_at: "2026-10-10T10:00:00Z",
    is_reversal: false,
  };
}

function page(
  actuals: ReturnType<typeof line>[],
  total: number,
  amount: number,
): BudgetCockpitDrilldown {
  return {
    actuals,
    commitments: [],
    actuals_total: total,
    actuals_amount: amount,
    limit: 200,
    offset: 0,
  } as unknown as BudgetCockpitDrilldown;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function sheet(r: BudgetCockpitRow) {
  return (
    <CockpitDrilldownSheet
      row={r}
      fiscalYear={2026}
      budgetVersionId={7}
      currency="DZD"
      onClose={vi.fn()}
    />
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("CockpitDrilldownSheet", () => {
  it("drops a late page of the previous cost centre", async () => {
    const latePageOfA = deferred<BudgetCockpitDrilldown>();
    svc.getBudgetCockpitDrilldown
      // Cost centre A: first page (1 of 2 lines).
      .mockResolvedValueOnce(page([line(1, "A-1")], 2, 20))
      // A's "load more": answered only after B is on screen.
      .mockReturnValueOnce(latePageOfA.promise)
      // Cost centre B: its only line.
      .mockResolvedValueOnce(page([line(7, "B-7")], 1, 70));

    const { rerender } = render(sheet(row(1, "CC-A")));
    expect(await screen.findByText("A-1")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ds.state.loadMore" }));
    await waitFor(() => expect(svc.getBudgetCockpitDrilldown).toHaveBeenCalledTimes(2));

    rerender(sheet(row(2, "CC-B")));
    expect(await screen.findByText("B-7")).toBeInTheDocument();

    await act(async () => {
      latePageOfA.resolve(page([line(2, "A-2")], 2, 20));
      await latePageOfA.promise;
    });

    expect(screen.queryByText("A-2")).toBeNull();
    expect(screen.getByText("drill.actuals:1")).toBeInTheDocument();
    expect(screen.queryByText("drill.actuals:2")).toBeNull();
  });

  it("lets the new cost centre load more while an old page is still pending", async () => {
    const latePageOfA = deferred<BudgetCockpitDrilldown>();
    svc.getBudgetCockpitDrilldown
      .mockResolvedValueOnce(page([line(1, "A-1")], 2, 20))
      .mockReturnValueOnce(latePageOfA.promise)
      .mockResolvedValueOnce(page([line(7, "B-7")], 2, 140))
      .mockResolvedValueOnce(page([line(8, "B-8")], 2, 140));

    const { rerender } = render(sheet(row(1, "CC-A")));
    await screen.findByText("A-1");
    fireEvent.click(screen.getByRole("button", { name: "ds.state.loadMore" }));
    await waitFor(() => expect(svc.getBudgetCockpitDrilldown).toHaveBeenCalledTimes(2));

    rerender(sheet(row(2, "CC-B")));
    await screen.findByText("B-7");
    // A's request is still pending, yet B's "load more" works.
    fireEvent.click(screen.getByRole("button", { name: "ds.state.loadMore" }));
    expect(await screen.findByText("B-8")).toBeInTheDocument();
    expect(svc.getBudgetCockpitDrilldown).toHaveBeenLastCalledWith(
      expect.objectContaining({ cost_center_id: 2, offset: 1 }),
    );
  });
});

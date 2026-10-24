/**
 * negative-stock-service: IPC contract of the stock to reconcile (backend `inventory::negative_stock`).
 *
 * The list and the sync-centre summary are parsed against the Rust shape: a response missing the
 * over-issue / over-receipt fields or carrying an unknown item kind is rejected, so a Rust/TS drift
 * fails loudly instead of hiding stock to reconcile. A movement type added later is still shown.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { getNegativeStockSummary, listNegativeStockReviews } from "../negative-stock-service";

const TS = "2026-10-23T08:00:00Z";

function overIssue(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    kind: "over_issue",
    article_id: 21,
    article_code: "A-21",
    article_name: "Bearing",
    location_id: 10,
    location_code: "BIN",
    warehouse_code: "MAIN",
    work_order_code: "OT-0042",
    reservation_ref: null,
    purchase_order_number: null,
    expected_qty: 5,
    actual_qty: 10,
    excess_qty: 5,
    movements: [
      {
        movement_sync_id: "i1",
        movement_type: "ISSUE",
        document_ref: null,
        posted_by: "Alice",
        posted_at: TS,
        quantity: 5,
      },
    ],
    closure_kind: "return",
    closed_by_return: {
      movement_sync_id: "r1",
      movement_type: "RETURN",
      document_ref: null,
      posted_by: "Carol",
      posted_at: TS,
      quantity: 5,
    },
    closed_by: null,
    on_hand_qty: 5,
    reserved_qty: 0,
    available_qty: 5,
    ...overrides,
  };
}

function reviewList(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    items: [],
    double_counts: [],
    closed_double_counts: [],
    double_postings: [],
    closed_double_postings: [],
    over_quantities: [],
    closed_over_quantities: [overIssue()],
    over_reservations: [],
    pending_counts: [],
    self_approval_allowed: false,
    ...overrides,
  };
}

const SUMMARY = {
  negative_count: 0,
  double_count_count: 0,
  double_posting_count: 0,
  over_quantity_count: 1,
  over_reserved_count: 0,
};

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("list of the stock to reconcile", () => {
  it("reads a closed over-issue with the return that closed it", async () => {
    mockInvoke.mockResolvedValueOnce(reviewList());
    const list = await listNegativeStockReviews(true);
    expect(mockInvoke).toHaveBeenCalledWith("list_negative_stock_reviews", {
      includeClosed: true,
    });
    expect(list.closed_over_quantities[0]?.closure_kind).toBe("return");
    expect(list.closed_over_quantities[0]?.closed_by_return?.posted_by).toBe("Carol");
  });

  it("keeps a movement type it does not know instead of rejecting the list", async () => {
    mockInvoke.mockResolvedValueOnce(
      reviewList({
        over_quantities: [
          overIssue({
            closure_kind: null,
            closed_by_return: null,
            movements: [
              {
                movement_sync_id: "x1",
                movement_type: "ISSUE_NEW_KIND",
                document_ref: null,
                posted_by: null,
                posted_at: TS,
                quantity: 5,
              },
            ],
          }),
        ],
      }),
    );
    const list = await listNegativeStockReviews(false);
    expect(list.over_quantities[0]?.movements[0]?.movement_type).toBe("ISSUE_NEW_KIND");
  });

  it("rejects a list without its over-issues and over-receipts", async () => {
    const without = reviewList();
    delete without["over_quantities"];
    mockInvoke.mockResolvedValueOnce(without);
    await expect(listNegativeStockReviews(false)).rejects.toThrow();
  });

  it.each(["closure_kind", "closed_by_return"])(
    "rejects a closed over-issue without its %s key",
    async (key) => {
      const item = overIssue();
      delete item[key];
      mockInvoke.mockResolvedValueOnce(reviewList({ closed_over_quantities: [item] }));
      await expect(listNegativeStockReviews(true)).rejects.toThrow();
    },
  );

  it("rejects a closing count without its approved_by key", async () => {
    const count: Record<string, unknown> = {
      count_sync_id: "c1",
      session_code: "CC-9",
      path: "count_session",
      counted_by: "Alice",
      approved_by: null,
      device_code: "PC-1",
      counted_at: TS,
      system_qty: 10,
      counted_qty: 5,
      variance_qty: -5,
    };
    const closedByCount = overIssue({
      closure_kind: "count",
      closed_by_return: null,
      closed_by: count,
    });
    mockInvoke.mockResolvedValueOnce(reviewList({ closed_over_quantities: [closedByCount] }));
    const list = await listNegativeStockReviews(true);
    expect(list.closed_over_quantities[0]?.closed_by?.approved_by).toBeNull();

    delete count["approved_by"];
    mockInvoke.mockResolvedValueOnce(reviewList({ closed_over_quantities: [closedByCount] }));
    await expect(listNegativeStockReviews(true)).rejects.toThrow();
  });

  it("rejects an item of an unknown kind", async () => {
    mockInvoke.mockResolvedValueOnce(
      reviewList({ closed_over_quantities: [overIssue({ kind: "other" })] }),
    );
    await expect(listNegativeStockReviews(true)).rejects.toThrow();
  });
});

describe("sync-centre summary", () => {
  it("reads the over-issues and over-receipts to reconcile", async () => {
    mockInvoke.mockResolvedValueOnce(SUMMARY);
    const summary = await getNegativeStockSummary();
    expect(mockInvoke).toHaveBeenCalledWith("get_negative_stock_summary", undefined);
    expect(summary.over_quantity_count).toBe(1);
  });

  it("rejects a summary without the over-quantity count", async () => {
    const without: Record<string, unknown> = { ...SUMMARY };
    delete without["over_quantity_count"];
    mockInvoke.mockResolvedValueOnce(without);
    await expect(getNegativeStockSummary()).rejects.toThrow();
  });
});

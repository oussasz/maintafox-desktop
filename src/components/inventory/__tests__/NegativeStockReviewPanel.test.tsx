import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { NegativeStockReviewPanel } from "@/components/inventory/NegativeStockReviewPanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import frInventory from "@/i18n/locale-data/fr/inventory.json";
import type * as NegativeStockService from "@/services/negative-stock-service";
import { useSessionStore } from "@/store/session-store";

const mocks = vi.hoisted(() => ({
  listNegativeStockReviews: vi.fn(),
  resolveNegativeStockByCount: vi.fn(),
  approvePendingNegativeStockCount: vi.fn(),
  getNegativeStockSummary: vi.fn(),
}));
const lookupMocks = vi.hoisted(() => ({ getLookupValues: vi.fn() }));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/services/negative-stock-service", async (importOriginal) => ({
  ...(await importOriginal<typeof NegativeStockService>()),
  ...mocks,
}));
vi.mock("@/services/lookup-service", () => lookupMocks);

const TS = "2026-10-20T08:00:00Z";

function item(overrides: Record<string, unknown> = {}) {
  return {
    id: 1,
    article_id: 21,
    article_code: "A-21",
    article_name: "Bearing",
    location_id: 10,
    location_code: "BIN",
    warehouse_code: "MAIN",
    status: "open",
    opened_at: TS,
    opening_transaction_id: 99,
    opening_movement_type: "ISSUE",
    closed_at: null,
    closure_kind: null,
    closing_transaction_id: null,
    closing_movement_type: null,
    on_hand_qty: -5,
    reserved_qty: 0,
    available_qty: -5,
    ...overrides,
  };
}

describe("NegativeStockReviewPanel", () => {
  beforeAll(async () => {
    Element.prototype.scrollIntoView = vi.fn();
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory", "common"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory, common: enCommon } },
        interpolation: { escapeValue: false },
      });
    } else {
      i18next.addResourceBundle("en", "inventory", enInventory, true, true);
      i18next.addResourceBundle("en", "common", enCommon, true, true);
    }
  });

  beforeEach(() => {
    Object.values(mocks).forEach((fn) => fn.mockReset());
    lookupMocks.getLookupValues.mockReset();
    lookupMocks.getLookupValues.mockResolvedValue([
      {
        id: 1,
        code: "COUNT_ERROR",
        label: "Erreur de comptage",
        fr_label: "Erreur de comptage",
        en_label: "Count error",
        color: null,
        is_active: 1,
      },
    ]);
  });

  it("flags a location below zero once, and shows over-reservations as a warning only", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [item()],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [
        {
          article_id: 22,
          article_code: "A-22",
          article_name: "Seal",
          location_id: 11,
          location_code: "BIN2",
          warehouse_code: "MAIN",
          on_hand_qty: 5,
          reserved_qty: 7,
          available_qty: -2,
        },
      ],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);

    expect(await screen.findByText("Negative — to reconcile")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Record a count" })).toHaveLength(1);
    expect(screen.getByText("Reservations above stock (warning)")).toBeInTheDocument();
    expect(screen.getByText(/A-22 — Seal at MAIN\/BIN2/)).toBeInTheDocument();
    // Nothing to reject or accept as is: the count is the one action.
    expect(screen.queryByRole("button", { name: /reject|accept/i })).not.toBeInTheDocument();
    expect(mocks.listNegativeStockReviews).toHaveBeenCalledWith(false);
  });

  it("records a count only with a counted quantity, a variance reason and an 8-character reason", async () => {
    mocks.listNegativeStockReviews.mockResolvedValueOnce({
      items: [item()],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    mocks.resolveNegativeStockByCount.mockResolvedValue({
      session_code: "CC-1",
      counted_qty: 0,
      variance_qty: 5,
      review: item({ status: "closed", closure_kind: "count", closed_at: TS, on_hand_qty: 0 }),
      pending_approval: false,
      self_approved: false,
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Record a count" }));

    const confirm = screen.getByRole("button", { name: "Record the count" });
    fireEvent.change(screen.getByLabelText("Quantity counted"), { target: { value: "0" } });
    fireEvent.click(screen.getByLabelText("Variance reason"));
    fireEvent.click(await screen.findByRole("option", { name: "Count error" }));
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "  abcdefg  " } });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: " Bin empty " } });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(mocks.resolveNegativeStockByCount).toHaveBeenCalledWith({
        article_id: 21,
        location_id: 10,
        counted_qty: 0,
        variance_reason_code: "COUNT_ERROR",
        reason: "Bin empty",
      }),
    );
    expect(await screen.findByText("No location below zero.")).toBeInTheDocument();
  });

  it("shows how a reconciled item was closed, by a count or regularised by receipt", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 2,
          status: "closed",
          closure_kind: "receipt",
          closed_at: TS,
          closing_movement_type: "GR_ACCEPT",
        }),
      ],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    await waitFor(() => expect(mocks.listNegativeStockReviews).toHaveBeenLastCalledWith(true));
    expect(
      await screen.findByText(/Regularised by receipt on .* \(Goods receipt\)/),
    ).toBeInTheDocument();
  });

  it("names both counts of a double count and offers a new count", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [
        {
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          on_hand_qty: 5,
          reserved_qty: 0,
          available_qty: 5,
          closed_by_reversal: null,
          counts: [
            {
              count_sync_id: "s1",
              path: "reconciliation",
              system_qty: -5,
              session_code: "CC-1",
              counted_by: "Alice",
              approved_by: "Alice",
              device_code: "7KQ2",
              counted_at: TS,
              counted_qty: 0,
              variance_qty: 5,
            },
            {
              count_sync_id: "s2",
              path: "count_session",
              system_qty: -5,
              session_code: "CC-2",
              counted_by: "Bob",
              approved_by: "Bob",
              device_code: "M3XZ",
              counted_at: TS,
              counted_qty: 0,
              variance_qty: 5,
            },
          ],
        },
      ],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    expect(await screen.findByText("Double count — to verify")).toBeInTheDocument();
    expect(
      screen.getByText(/Alice on PC 7KQ2, .*, by stock-to-reconcile list: counted 0/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Bob on PC M3XZ, .*, by count session CC-2: counted 0/),
    ).toBeInTheDocument();
    expect(screen.queryByText("No location below zero.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Record a count" }));
    expect(await screen.findByRole("button", { name: "Record the count" })).toBeDisabled();
  });

  it("names the session and both movements of a count posted twice and offers a new count", async () => {
    const posting = {
      kind: "posted_twice",
      article_id: 21,
      article_code: "A-21",
      article_name: "Bearing",
      location_id: 10,
      location_code: "BIN",
      warehouse_code: "MAIN",
      session_code: "CC-7",
      movement_type: "COUNT_ADJUST",
      movements: [
        {
          movement_sync_id: "m1",
          posted_by: "Alice",
          posted_at: TS,
          quantity: -3,
          device_code: null,
        },
        {
          movement_sync_id: "m2",
          posted_by: "Bob",
          posted_at: TS,
          quantity: -3,
          device_code: null,
        },
      ],
      variance_qty: -3,
      excess_qty: -3,
      closed_by: null,
      on_hand_qty: 4,
      reserved_qty: 0,
      available_qty: 4,
    };
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [posting],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    expect(await screen.findByText("Count posted twice — to verify")).toBeInTheDocument();
    expect(screen.getByText(/Count CC-7 was posted to stock more than once/)).toBeInTheDocument();
    expect(screen.getByText(/^Alice, .*: .?3 /)).toBeInTheDocument();
    expect(screen.getByText(/^Bob, .*: .?3 /)).toBeInTheDocument();
    expect(screen.queryByText("No location below zero.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Record a count" }));
    expect(await screen.findByRole("button", { name: "Record the count" })).toBeDisabled();
  });

  it("names both versions of a divergent count and the PC of the one replaced", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [
        {
          kind: "divergent",
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          session_code: "CC-8",
          movement_type: "COUNT_ADJUST",
          movements: [
            {
              movement_sync_id: "m1",
              posted_by: "Alice",
              posted_at: TS,
              quantity: -3,
              device_code: null,
            },
            {
              movement_sync_id: "m1",
              posted_by: "Bob",
              posted_at: TS,
              quantity: -4,
              device_code: "M3XZ",
            },
          ],
          variance_qty: -3,
          excess_qty: -1,
          closed_by: null,
          on_hand_qty: 7,
          reserved_qty: 0,
          available_qty: 7,
        },
      ],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    expect(await screen.findByText("Divergent count — to verify")).toBeInTheDocument();
    expect(
      screen.getByText(/Count CC-8 was posted on two PCs with different quantities/),
    ).toBeInTheDocument();
    expect(screen.getByText(/^Alice, .*: .?3 \(/)).toBeInTheDocument();
    expect(screen.getByText(/^Bob on PC M3XZ, .*: .?4$/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Record a count" })).toBeInTheDocument();
  });

  it("labels a manual entry as an adjustment, not a receipt", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 3,
          status: "closed",
          closure_kind: "adjustment",
          closed_at: TS,
          closing_movement_type: "ADJUST_IN",
        }),
      ],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(/Reconciled by an adjustment with its reason on .*\(Stock entry\)/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Regularised by receipt/)).not.toBeInTheDocument();
  });

  it("says when an entry made before the shortage was visible closed the item", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 4,
          status: "closed",
          closure_kind: "prior_entry",
          closed_at: TS,
          closing_movement_type: "ADJUST_IN",
        }),
      ],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(/Regularised by an earlier entry on .*no reason asked/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Reconciled by an adjustment/)).not.toBeInTheDocument();
    expect(frInventory.negativeStock.closure.prior_entry).toMatch(
      /^Régularisé par saisie antérieure/,
    );
  });

  it("shows a double count closed by the reversal of one of its counts", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [
        {
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          on_hand_qty: 0,
          reserved_qty: 0,
          available_qty: 0,
          closed_by_reversal: {
            count_sync_id: "s1",
            session_code: "CC-1",
            reversed_by: "Alice",
            reversed_at: TS,
          },
          counts: [],
        },
      ],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(/closed by the reversal of count CC-1 \(Alice, .+\)/),
    ).toBeInTheDocument();
    expect(frInventory.negativeStock.doubleCount.closedByReversal).toMatch(
      /^clos par annulation du comptage/,
    );
  });

  it("names the work order, the purchase order and every movement of an over-issue and an over-receipt", async () => {
    const over = (overrides: Record<string, unknown>) => ({
      article_id: 21,
      article_code: "A-21",
      article_name: "Bearing",
      location_id: 10,
      location_code: "BIN",
      warehouse_code: "MAIN",
      work_order_code: null,
      reservation_ref: null,
      purchase_order_number: null,
      expected_qty: 5,
      actual_qty: 10,
      excess_qty: 5,
      closure_kind: null,
      closed_by_return: null,
      closed_by: null,
      on_hand_qty: 0,
      reserved_qty: 0,
      available_qty: 0,
      ...overrides,
    });
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [
        over({
          kind: "over_issue",
          work_order_code: "OT-0042",
          movements: [
            {
              movement_sync_id: "i1",
              movement_type: "ISSUE",
              document_ref: null,
              posted_by: "Alice",
              posted_at: TS,
              quantity: 5,
            },
            {
              movement_sync_id: "i2",
              movement_type: "ISSUE",
              document_ref: null,
              posted_by: "Bob",
              posted_at: TS,
              quantity: 5,
            },
          ],
        }),
        over({
          kind: "over_receipt",
          purchase_order_number: "PO-77",
          on_hand_qty: 10,
          available_qty: 10,
          movements: [
            {
              movement_sync_id: "g1",
              movement_type: "GR_ACCEPT",
              document_ref: "GR-1",
              posted_by: null,
              posted_at: TS,
              quantity: 5,
            },
          ],
        }),
      ],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    expect(await screen.findByText("Over-issue — to reconcile")).toBeInTheDocument();
    expect(screen.getByText("Over-receipt — to reconcile")).toBeInTheDocument();
    expect(
      screen.getByText(
        /^Reservation for work order OT-0042: 5 reserved, 10 issued in all .*5 too many/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/^Purchase order PO-77: 5 ordered, 10 received in all .*5 too many/),
    ).toBeInTheDocument();
    expect(screen.getByText(/^Issue to work order — Alice, .*: 5$/)).toBeInTheDocument();
    expect(screen.getByText(/^Issue to work order — Bob, .*: 5$/)).toBeInTheDocument();
    expect(screen.getByText(/^Goods receipt GR-1 — Unknown user, .*: 5$/)).toBeInTheDocument();
    expect(
      screen.getByText(/^If these receipts were rejected in full at inspection, no stock moved/),
    ).toBeInTheDocument();
    expect(screen.queryByText("No location below zero.")).not.toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: "Record a count" });
    expect(buttons).toHaveLength(2);
    fireEvent.click(buttons[0] as HTMLElement);
    expect(await screen.findByRole("button", { name: "Record the count" })).toBeDisabled();
    expect(frInventory.negativeStock.overQuantity.flagKind.over_issue).toBe(
      "Sur-sortie — à rapprocher",
    );
    expect(frInventory.negativeStock.overQuantity.flagKind.over_receipt).toBe(
      "Sur-réception — à rapprocher",
    );
  });

  it("shows an over-receipt closed by a count with the closed items", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [
        {
          kind: "over_receipt",
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          work_order_code: null,
          reservation_ref: null,
          purchase_order_number: "PO-77",
          expected_qty: 5,
          actual_qty: 10,
          excess_qty: 5,
          movements: [],
          closure_kind: "count",
          closed_by_return: null,
          closed_by: {
            count_sync_id: "c1",
            session_code: "CC-9",
            path: "reconciliation",
            counted_by: "Alice",
            approved_by: "Alice",
            device_code: "7KQ2",
            counted_at: TS,
            system_qty: 10,
            counted_qty: 5,
            variance_qty: -5,
          },
          on_hand_qty: 5,
          reserved_qty: 0,
          available_qty: 5,
        },
      ],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(
        /Over-receipt — to reconcile · PO-77 · Closed by count CC-9 from the stock-to-reconcile list: counted by Alice, .+, variance approved by Alice$/,
      ),
    ).toBeInTheDocument();
  });

  it("shows an over-issue regularised by return, and one closed by a count session, with the closed items", async () => {
    const closedIssue = (overrides: Record<string, unknown>) => ({
      kind: "over_issue",
      article_id: 21,
      article_code: "A-21",
      article_name: "Bearing",
      location_id: 10,
      location_code: "BIN",
      warehouse_code: "MAIN",
      work_order_code: null,
      reservation_ref: null,
      purchase_order_number: null,
      expected_qty: 5,
      actual_qty: 10,
      excess_qty: 5,
      movements: [],
      closure_kind: null,
      closed_by_return: null,
      closed_by: null,
      on_hand_qty: 5,
      reserved_qty: 0,
      available_qty: 5,
      ...overrides,
    });
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [
        closedIssue({
          closure_kind: "return",
          closed_by_return: {
            movement_sync_id: "r1",
            movement_type: "RETURN",
            document_ref: null,
            posted_by: "Carol",
            posted_at: TS,
            quantity: 5,
          },
        }),
        closedIssue({
          reservation_ref: "RES-12",
          closure_kind: "count",
          closed_by: {
            count_sync_id: "c2",
            session_code: "CC-31",
            path: "count_session",
            counted_by: "Dan",
            approved_by: null,
            device_code: "M3XZ",
            counted_at: TS,
            system_qty: 5,
            counted_qty: 5,
            variance_qty: 0,
          },
        }),
      ],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(
        /Over-issue — to reconcile · Reservation without work order · Regularised by return: 5 returned by Carol, .+$/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Over-issue — to reconcile · Reservation RES-12 · Closed by count CC-31 from a count session or inventory campaign: counted by Dan, (?!.*approved).+$/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Record a count" })).not.toBeInTheDocument();
    expect(frInventory.negativeStock.overQuantity.documentNone).toBe("Réservation sans OT");
    expect(frInventory.negativeStock.overQuantity.closedByReturn).toMatch(/^Régularisé par retour/);
    expect(frInventory.negativeStock.overQuantity.closedPath.count_session).toBe(
      "d'une session de comptage ou d'une campagne d'inventaire",
    );
  });

  it("shows a closed count posted twice with the count's path and approver", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [
        {
          kind: "posted_twice",
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          session_code: "CC-4",
          movement_type: "COUNT_ADJUST",
          movements: [],
          variance_qty: -2,
          excess_qty: -2,
          closed_by: {
            count_sync_id: "c3",
            session_code: "CC-12",
            path: "count_session",
            counted_by: "Erin",
            approved_by: "Frank",
            device_code: "Q8LW",
            counted_at: TS,
            system_qty: 6,
            counted_qty: 8,
            variance_qty: 2,
          },
          on_hand_qty: 8,
          reserved_qty: 0,
          available_qty: 8,
        },
      ],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(
        /Count posted twice — to verify · Closed by count CC-12 from a count session or inventory campaign: counted by Erin, .+, variance approved by Frank$/,
      ),
    ).toBeInTheDocument();
  });

  it("has every text in French too", () => {
    const keys = (o: Record<string, unknown>, p = ""): string[] =>
      Object.entries(o).flatMap(([k, v]) =>
        v && typeof v === "object" ? keys(v as Record<string, unknown>, `${p}${k}.`) : [`${p}${k}`],
      );
    const en = keys({ negativeStock: enInventory.negativeStock, errors: enInventory.errors });
    const fr = keys({ negativeStock: frInventory.negativeStock, errors: frInventory.errors });
    expect(fr).toEqual(en);
    expect(frInventory.negativeStock.flag).toBe("Négatif — à rapprocher");
    expect(frInventory.negativeStock.closure.receipt).toMatch(/^Régularisé par réception/);
  });

  it("shows a count waiting for a second person and approves it with an 8-character note", async () => {
    const pending = {
      line_id: 7,
      row_version: 3,
      session_code: "CC-7",
      article_id: 21,
      article_code: "A-21",
      article_name: "Bearing",
      location_id: 10,
      location_code: "BIN",
      warehouse_code: "MAIN",
      counted_by_id: 4,
      counted_by: "Karim",
      counted_at: TS,
      system_qty: -5,
      counted_qty: 2,
      variance_qty: 7,
      variance_reason_code: "COUNT_ERROR",
    };
    const empty = {
      items: [],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    };
    mocks.listNegativeStockReviews.mockResolvedValueOnce({
      ...empty,
      items: [item()],
      pending_counts: [pending],
    });
    mocks.listNegativeStockReviews.mockResolvedValue(empty);
    mocks.approvePendingNegativeStockCount.mockResolvedValue({
      session_code: "CC-7",
      counted_qty: 2,
      variance_qty: 7,
      review: item({ status: "closed", closure_kind: "count", closed_at: TS, on_hand_qty: 2 }),
      pending_approval: false,
      self_approved: false,
    });
    render(<NegativeStockReviewPanel />);

    // The waiting count is listed, and its item says so.
    expect(await screen.findAllByText("Count waiting for approval")).toHaveLength(2);
    expect(screen.getByText(/Counted by Karim on .* \(CC-7\)/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve the count" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Approve and post" });
    fireEvent.change(within(dialog).getByLabelText("Approval note"), {
      target: { value: " court " },
    });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Approval note"), {
      target: { value: "Recompté avec le magasinier" },
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(mocks.approvePendingNegativeStockCount).toHaveBeenCalledWith({
        line_id: 7,
        expected_row_version: 3,
        reason: "Recompté avec le magasinier",
      }),
    );
    expect(await screen.findByText("No location below zero.")).toBeInTheDocument();
  });

  it("does not offer a waiting count's approval to its counter, except under the opt-out", async () => {
    const pending = {
      line_id: 7,
      row_version: 3,
      session_code: "CC-7",
      article_id: 21,
      article_code: "A-21",
      article_name: "Bearing",
      location_id: 10,
      location_code: "BIN",
      warehouse_code: "MAIN",
      counted_by_id: 4,
      counted_by: "Karim",
      counted_at: TS,
      system_qty: -5,
      counted_qty: 2,
      variance_qty: 7,
      variance_reason_code: "COUNT_ERROR",
    };
    const list = (selfApprovalAllowed: boolean) => ({
      items: [item()],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [pending],
      self_approval_allowed: selfApprovalAllowed,
    });
    useSessionStore.setState({ info: { user_id: 4 } as never });
    try {
      mocks.listNegativeStockReviews.mockResolvedValue(list(false));
      const { unmount } = render(<NegativeStockReviewPanel />);
      expect(
        await screen.findByText("You made this count: another person must approve it."),
      ).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Approve the count" })).toBeNull();
      expect(frInventory.negativeStock.pending.notByCounter).toBe(
        "Vous avez effectué ce comptage : une autre personne doit l'approuver.",
      );
      unmount();

      mocks.listNegativeStockReviews.mockResolvedValue(list(true));
      render(<NegativeStockReviewPanel />);
      expect(await screen.findByRole("button", { name: "Approve the count" })).toBeInTheDocument();
    } finally {
      useSessionStore.setState({ info: null });
    }
  });

  it("says a count above the tolerance waits for a second person", async () => {
    const list = {
      items: [item()],
      double_counts: [],
      closed_double_counts: [],
      double_postings: [],
      closed_double_postings: [],
      over_quantities: [],
      closed_over_quantities: [],
      over_reservations: [],
      pending_counts: [],
    };
    mocks.listNegativeStockReviews.mockResolvedValue(list);
    mocks.resolveNegativeStockByCount.mockResolvedValue({
      session_code: "CC-8",
      counted_qty: 0,
      variance_qty: 5,
      review: item(),
      pending_approval: true,
      self_approved: false,
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Record a count" }));
    fireEvent.change(screen.getByLabelText("Quantity counted"), { target: { value: "0" } });
    fireEvent.click(screen.getByLabelText("Variance reason"));
    fireEvent.click(await screen.findByRole("option", { name: "Count error" }));
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "Bin empty" } });
    fireEvent.click(screen.getByRole("button", { name: "Record the count" }));
    expect(
      await screen.findByText(/waits for a second person's approval\. Nothing moved yet\./),
    ).toBeInTheDocument();
  });
});

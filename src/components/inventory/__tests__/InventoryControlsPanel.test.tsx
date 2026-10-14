import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { InventoryControlsPanel } from "@/components/inventory/InventoryControlsPanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";

const mocks = vi.hoisted(() => ({
  approveInventoryCountLine: vi.fn(),
  createInventoryCountSession: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryCountLines: vi.fn(),
  listInventoryCountSessions: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryReconciliationFindings: vi.fn(),
  listInventoryReconciliationRuns: vi.fn(),
  listInventoryTransactions: vi.fn(),
  listInventoryWarehouses: vi.fn(),
  postInventoryCountSession: vi.fn(),
  reverseInventoryCountSession: vi.fn(),
  runInventoryReconciliation: vi.fn(),
  transitionInventoryCountSession: vi.fn(),
  upsertInventoryCountLine: vi.fn(),
}));

const lookupMocks = vi.hoisted(() => ({ getLookupValues: vi.fn() }));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 7 } }),
}));

vi.mock("@/services/inventory-service", () => mocks);
vi.mock("@/services/lookup-service", () => lookupMocks);

const TS = "2026-01-01T00:00:00Z";

function session(overrides: Record<string, unknown> = {}) {
  return {
    id: 100,
    session_code: "CC-100",
    warehouse_id: 1,
    location_id: 10,
    status: "draft",
    critical_abs_threshold: 5,
    submitted_by_id: null,
    submitted_at: null,
    posted_by_id: null,
    posted_at: null,
    reversed_by_id: null,
    reversed_at: null,
    reversal_reason: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function countLine(overrides: Record<string, unknown> = {}) {
  return {
    id: 1,
    session_id: 100,
    article_id: 21,
    article_code: "A-21",
    article_name: "Bearing",
    warehouse_id: 1,
    location_id: 10,
    location_code: "BIN",
    system_qty: 10,
    counted_qty: 2,
    variance_qty: -8,
    variance_reason_code: null,
    is_critical: 1,
    approval_required: 1,
    approved_by_id: null,
    approved_by_name: null,
    approved_at: null,
    approval_note: null,
    posted_transaction_id: null,
    reversed_transaction_id: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

async function selectSession(label: string) {
  fireEvent.click(screen.getByLabelText("Count session"));
  fireEvent.click(await screen.findByText(label));
}

describe("InventoryControlsPanel", () => {
  beforeAll(async () => {
    // Radix Select scrolls the selected option into view; jsdom has no layout.
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
    }
  });

  beforeEach(() => {
    Object.values(mocks).forEach((fn) => fn.mockReset());
    lookupMocks.getLookupValues.mockReset();
    lookupMocks.getLookupValues.mockImplementation(async (domain: string) =>
      domain === "inventory.variance_reason"
        ? [
            {
              id: 1,
              code: "MISCOUNT",
              label: "Erreur de comptage",
              fr_label: "Erreur de comptage",
              en_label: "Miscount",
              color: null,
              is_active: 1,
            },
          ]
        : [],
    );
    mocks.listInventoryWarehouses.mockResolvedValue([
      { id: 1, code: "MAIN", name: "Main", is_active: 1 },
    ]);
    mocks.listInventoryLocations.mockResolvedValue([
      { id: 10, warehouse_id: 1, warehouse_code: "MAIN", code: "BIN", is_active: 1 },
    ]);
    mocks.listInventoryArticles.mockResolvedValue([
      { id: 21, article_code: "A-21", article_name: "Bearing", is_active: 1 },
    ]);
    mocks.listInventoryCountSessions.mockResolvedValue([]);
    mocks.listInventoryCountLines.mockResolvedValue([]);
    mocks.listInventoryReconciliationRuns.mockResolvedValue([]);
    mocks.listInventoryReconciliationFindings.mockResolvedValue([]);
    mocks.listInventoryTransactions.mockResolvedValue([]);
    mocks.createInventoryCountSession.mockResolvedValue(session());
    mocks.approveInventoryCountLine.mockResolvedValue(countLine());
  });

  it("creates a count session with the chosen scope and critical threshold", async () => {
    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryWarehouses).toHaveBeenCalled());

    fireEvent.click(screen.getByLabelText("Warehouse"));
    fireEvent.click(await screen.findByText("MAIN - Main"));
    fireEvent.click(screen.getByLabelText("Location (optional)"));
    fireEvent.click(await screen.findByText("MAIN/BIN"));
    fireEvent.change(screen.getByLabelText("Critical variance (quantity)"), {
      target: { value: "3" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create count session" }));

    await waitFor(() => {
      expect(mocks.createInventoryCountSession).toHaveBeenCalledWith(
        expect.objectContaining({ warehouse_id: 1, location_id: 10, critical_abs_threshold: 3 }),
      );
    });
  });

  it("approves the variance line the user chose, not the first pending one", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "submitted" })]);
    mocks.listInventoryCountLines.mockResolvedValue([
      countLine({ id: 1, article_code: "A-21", location_code: "BIN" }),
      countLine({ id: 2, article_code: "A-22", article_name: "Seal", location_code: "BIN-2" }),
    ]);

    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Submitted)");

    const secondRow = (await screen.findByText("A-22")).closest("tr");
    if (!secondRow) throw new Error("expected the A-22 row");
    fireEvent.click(within(secondRow).getByRole("button", { name: "Approve variance" }));

    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Approve variance" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Reviewer evidence"), {
      target: { value: "Recounted twice" },
    });
    fireEvent.click(confirm);

    await waitFor(() => {
      expect(mocks.approveInventoryCountLine).toHaveBeenCalledWith(
        expect.objectContaining({ line_id: 2, reviewer_evidence: "Recounted twice" }),
      );
    });
    expect(mocks.approveInventoryCountLine).toHaveBeenCalledTimes(1);
  });

  it("shows the reviewer's name, never a raw user id", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "approved" })]);
    mocks.listInventoryCountLines.mockResolvedValue([
      countLine({
        approved_by_id: 42,
        approved_by_name: "Karim Benali",
        approved_at: "2026-02-03T10:00:00Z",
        approval_note: "Recount",
      }),
    ]);

    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Approved)");

    expect(await screen.findByText(/Approved by Karim Benali/)).toBeInTheDocument();
    expect(screen.queryByText(/#42/)).not.toBeInTheDocument();
  });

  it("asks for confirmation before reversing a posted count", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "posted" })]);
    mocks.reverseInventoryCountSession.mockResolvedValue(session({ status: "reversed" }));

    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Posted)");

    fireEvent.change(screen.getByLabelText("Reversal reason"), {
      target: { value: "Wrong bin counted" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Reverse posting" }));
    expect(mocks.reverseInventoryCountSession).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Reverse posting" }));

    await waitFor(() => {
      expect(mocks.reverseInventoryCountSession).toHaveBeenCalledWith(
        expect.objectContaining({ session_id: 100, reason: "Wrong bin counted" }),
      );
    });
  });

  it("replaces the panel with an error and a retry when loading fails", async () => {
    mocks.listInventoryCountSessions.mockRejectedValueOnce(new Error("database locked"));

    render(<InventoryControlsPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("database locked");
    fireEvent.click(screen.getByRole("button", { name: enCommon.ds.state.retry }));

    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });
});

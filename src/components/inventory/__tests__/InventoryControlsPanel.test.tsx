import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
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

/** The panel links to Settings › Inventory: it lives inside the app's router. */
function renderPanel() {
  return render(
    <MemoryRouter>
      <InventoryControlsPanel />
    </MemoryRouter>,
  );
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

  it("creates a count session with the chosen scope; the tolerance comes from the settings", async () => {
    renderPanel();
    await waitFor(() => expect(mocks.listInventoryWarehouses).toHaveBeenCalled());

    fireEvent.click(screen.getByLabelText("Warehouse"));
    fireEvent.click(await screen.findByText("MAIN - Main"));
    fireEvent.click(screen.getByLabelText("Location (optional)"));
    fireEvent.click(await screen.findByText("MAIN/BIN"));
    // No tolerance is typed on this screen: it is a plant setting.
    expect(screen.queryByLabelText(/critical variance/i)).not.toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "Settings › Inventory" })[0]).toHaveAttribute(
      "href",
      "/settings?tab=general",
    );
    fireEvent.click(screen.getByRole("button", { name: "Create count session" }));

    await waitFor(() => {
      expect(mocks.createInventoryCountSession).toHaveBeenCalledWith(
        expect.objectContaining({ warehouse_id: 1, location_id: 10 }),
      );
    });
    expect(mocks.createInventoryCountSession.mock.calls[0]?.[0]).not.toHaveProperty(
      "critical_abs_threshold",
    );
  });

  it("shows the critical variance the selected session was opened with, read-only", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ critical_abs_threshold: 2.5 })]);
    renderPanel();
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Draft)");
    expect(await screen.findByTestId("inv-cc-session-threshold")).toHaveTextContent(
      // Number formatted in the plant's locale (the formatter's locale here is French).
      /This session's critical variance: 2[.,]5$/,
    );
  });

  it("runs the reconciliation without sending a tolerance", async () => {
    mocks.runInventoryReconciliation.mockResolvedValue({});
    renderPanel();
    await waitFor(() => expect(mocks.listInventoryWarehouses).toHaveBeenCalled());
    expect(screen.queryByLabelText(/break threshold/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run reconciliation" }));
    await waitFor(() =>
      expect(mocks.runInventoryReconciliation).toHaveBeenCalledWith({ actor_id: 7 }),
    );
  });

  it("approves the variance line the user chose, not the first pending one", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "submitted" })]);
    mocks.listInventoryCountLines.mockResolvedValue([
      countLine({ id: 1, article_code: "A-21", location_code: "BIN" }),
      countLine({ id: 2, article_code: "A-22", article_name: "Seal", location_code: "BIN-2" }),
    ]);

    renderPanel();
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

  it("approves the count only once every variance is approved", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "submitted" })]);
    mocks.listInventoryCountLines.mockResolvedValue([countLine()]);

    renderPanel();
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Submitted)");
    await screen.findByText("A-21");
    expect(screen.getByRole("button", { name: "Approve count" })).toBeDisabled();
  });

  it("offers to approve the count once its variances are approved", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "submitted" })]);
    mocks.listInventoryCountLines.mockResolvedValue([
      countLine({
        approved_by_id: 42,
        approved_by_name: "Karim Benali",
        approved_at: TS,
        approval_note: "Recounted twice",
      }),
    ]);

    renderPanel();
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Submitted)");
    await screen.findByText("A-21");
    expect(screen.getByRole("button", { name: "Approve count" })).toBeEnabled();
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

    renderPanel();
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Approved)");

    expect(await screen.findByText(/Approved by Karim Benali/)).toBeInTheDocument();
    expect(screen.queryByText(/#42/)).not.toBeInTheDocument();
  });

  it("asks for confirmation before reversing a posted count", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "posted" })]);
    mocks.reverseInventoryCountSession.mockResolvedValue(session({ status: "reversed" }));

    renderPanel();
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

  it("asks for a reversal reason of at least 8 characters once trimmed", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([session({ status: "posted" })]);

    renderPanel();
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());
    await selectSession("CC-100 (Posted)");

    const reason = screen.getByLabelText("Reversal reason");
    fireEvent.change(reason, { target: { value: "  Erreur!  " } });
    expect(screen.getByRole("button", { name: "Reverse posting" })).toBeDisabled();
    fireEvent.change(reason, { target: { value: "Mauvais bac" } });
    expect(screen.getByRole("button", { name: "Reverse posting" })).toBeEnabled();
  });

  it("replaces the panel with an error and a retry when loading fails", async () => {
    mocks.listInventoryCountSessions.mockRejectedValueOnce(new Error("database locked"));

    renderPanel();

    expect(await screen.findByRole("alert")).toHaveTextContent("database locked");
    fireEvent.click(screen.getByRole("button", { name: enCommon.ds.state.retry }));

    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });
});

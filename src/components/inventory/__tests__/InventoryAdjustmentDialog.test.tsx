/**
 * InventoryAdjustmentDialog — an exit to a work order (SORTIE_OT) cannot be posted without
 * naming the work order, and the chosen OT code is sent as the movement's source reference.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { InventoryAdjustmentDialog } from "@/components/inventory/InventoryAdjustmentDialog";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import { useInventoryStore } from "@/stores/inventory-store";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  adjustInventoryStock: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryStockBalances: vi.fn(),
  projectInventoryStockImpact: vi.fn(),
}));
const wo = vi.hoisted(() => ({ listWos: vi.fn() }));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});
vi.mock("@/services/wo-service", () => wo);
vi.mock("@/services/lookup-service", () => ({
  getLookupValues: vi.fn(async () => [
    {
      id: 1,
      code: "ENTREE_ACHAT",
      label: "Entrée achat",
      fr_label: null,
      en_label: "Purchase receipt",
      color: null,
      is_active: 1,
    },
    {
      id: 2,
      code: "SORTIE_OT",
      label: "Sortie OT",
      fr_label: null,
      en_label: "Issue to work order",
      color: null,
      is_active: 1,
    },
  ]),
}));
vi.mock("@/components/inventory/StockImpactPreview", () => ({ StockImpactPreview: () => null }));

describe("InventoryAdjustmentDialog", () => {
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
    }
  });

  beforeEach(() => {
    Object.values(svc).forEach((fn) => fn.mockReset());
    wo.listWos.mockReset();
    useInventoryStore.setState({
      warehouses: [{ id: 2, code: "WH1", name: "Main", is_active: 1, created_at: "" }],
    });
    svc.listInventoryArticles.mockResolvedValue([
      { id: 4, article_code: "ART-001", article_name: "Bearing", is_active: 1 },
    ]);
    svc.listInventoryLocations.mockResolvedValue([
      {
        id: 3,
        warehouse_id: 2,
        warehouse_code: "WH1",
        code: "A-01",
        name: "Aisle A",
        is_active: 1,
      },
    ]);
    svc.listInventoryStockBalances.mockResolvedValue([
      { location_id: 3, on_hand_qty: 10, moving_avg_unit_cost: 4 },
    ]);
    svc.adjustInventoryStock.mockResolvedValue({});
    wo.listWos.mockResolvedValue({
      items: [{ id: 81, code: "OT-2026-0144", title: "Pump P-101 bearing change" }],
      total: 1,
    });
  });

  it("requires the work order for an exit to an OT and sends its code as source reference", async () => {
    const onSuccess = vi.fn();
    render(
      <InventoryAdjustmentDialog
        articleId={4}
        defaultWarehouseId={2}
        defaultLocationId={3}
        open
        onOpenChange={vi.fn()}
        onSuccess={onSuccess}
      />,
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Bearing/);

    fireEvent.click(within(dialog).getByRole("button", { name: "Exit" }));
    fireEvent.change(within(dialog).getByLabelText("Quantity"), { target: { value: "2" } });
    fireEvent.click(within(dialog).getByLabelText("Reason"));
    fireEvent.click(await screen.findByRole("option", { name: "Issue to work order" }));

    const validate = within(dialog).getByRole("button", { name: "Validate" });
    expect(validate).toBeDisabled();
    expect(
      within(dialog).getByText("Choose the work order the parts go to or come back from."),
    ).toBeInTheDocument();

    fireEvent.click(await within(dialog).findByRole("option", { name: /OT-2026-0144/ }));
    await waitFor(() => expect(validate).toBeEnabled());
    expect(wo.listWos).toHaveBeenCalledWith(
      expect.objectContaining({ status_codes: ["planning", "ready", "in_progress", "on_hold"] }),
    );

    fireEvent.click(validate);
    await waitFor(() =>
      expect(svc.adjustInventoryStock).toHaveBeenCalledWith(
        expect.objectContaining({
          article_id: 4,
          location_id: 3,
          delta_qty: -2,
          reason_code: "SORTIE_OT",
          source_ref: "OT-2026-0144",
        }),
        // Exits carry no cost of their own: valued at the weighted average by the backend.
        null,
      ),
    );
    expect(onSuccess).toHaveBeenCalled();
  });

  it("requires the unit cost of a purchase entry and sends it for the weighted average", async () => {
    render(
      <InventoryAdjustmentDialog
        articleId={4}
        defaultWarehouseId={2}
        defaultLocationId={3}
        open
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
      />,
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Bearing/);

    fireEvent.change(within(dialog).getByLabelText("Quantity"), { target: { value: "5" } });
    fireEvent.click(within(dialog).getByLabelText("Reason"));
    fireEvent.click(await screen.findByRole("option", { name: "Purchase receipt" }));

    const validate = within(dialog).getByRole("button", { name: "Validate" });
    expect(validate).toBeDisabled();
    expect(within(dialog).getByText("Enter the unit cost (zero or more).")).toBeInTheDocument();
    // The current location average is shown before the user types the new cost.
    expect(await within(dialog).findByText(/currently/)).toBeInTheDocument();

    fireEvent.change(within(dialog).getByLabelText(/^Unit cost/), { target: { value: "6.5" } });
    await waitFor(() => expect(validate).toBeEnabled());
    fireEvent.click(validate);

    await waitFor(() =>
      expect(svc.adjustInventoryStock).toHaveBeenCalledWith(
        expect.objectContaining({ delta_qty: 5, reason_code: "ENTREE_ACHAT" }),
        6.5,
      ),
    );
  });
});

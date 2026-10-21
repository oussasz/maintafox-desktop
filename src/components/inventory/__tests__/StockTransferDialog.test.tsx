/**
 * StockTransferDialog / StockTransferAction — the source is fixed and prefilled, the quantity is
 * capped at the available stock, the reason is required, a confirmation line names the move, the
 * coded refusals are shown translated, and the action needs the inventory-manage right.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { StockTransferAction } from "@/components/inventory/StockTransferAction";
import { StockTransferDialog } from "@/components/inventory/StockTransferDialog";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import type { InventoryArticleRow } from "@/services/inventory-service";
import type { InventoryStockBalance, StockLocation } from "@shared/ipc-types";

const svc = vi.hoisted(() => ({
  listInventoryLocations: vi.fn(),
  listInventoryLotStock: vi.fn(),
  transferInventoryStock: vi.fn(),
}));
vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<typeof InventoryService>();
  return { ...actual, ...svc };
});

const toast = vi.hoisted(() => ({ pushAppToast: vi.fn() }));
vi.mock("@/store/app-toast-store", () => toast);

// The coded refusals are translated by the shared error reason; its own tests cover the catalogue.
vi.mock("@/lib/app-error", () => ({
  appErrorReason: (err: { code?: string }) => `reason:${err.code ?? "?"}`,
}));

// A stable permission object: a new one per render would reset the dialog draft endlessly.
const perms = vi.hoisted(() => ({ allowed: true }));
vi.mock("@/hooks/use-permissions", () => {
  const value = {
    can: () => perms.allowed,
    canAny: () => perms.allowed,
    isLoading: false,
  };
  return { usePermissions: () => value };
});

const article = (over: Partial<InventoryArticleRow> = {}) =>
  ({
    id: 4,
    article_code: "ART-001",
    article_name: "Bearing 6204",
    unit_symbol: "ea",
    unit_label: "Each",
    requires_batch_tracking: 0,
    requires_serial_tracking: 0,
    ...over,
  }) as unknown as InventoryArticleRow;

const source = {
  id: 1,
  article_id: 4,
  warehouse_code: "MAIN",
  warehouse_name: "Main store",
  location_id: 3,
  location_code: "BIN-A",
  location_name: "Bin A",
  on_hand_qty: 8,
  reserved_qty: 3,
  available_qty: 5,
} as unknown as InventoryStockBalance;

const store = (id: number, code: string, active = 1) =>
  ({
    id,
    warehouse_id: 1,
    warehouse_code: "MAIN",
    code,
    name: `Name ${code}`,
    is_active: active,
  }) as StockLocation;

function renderDialog(over: Partial<InventoryArticleRow> = {}) {
  const onTransferred = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <StockTransferDialog
      open
      onOpenChange={onOpenChange}
      article={article(over)}
      source={source}
      onTransferred={onTransferred}
    />,
  );
  return { onTransferred, onOpenChange };
}

async function chooseDestination(label: RegExp) {
  fireEvent.keyDown(await screen.findByRole("combobox", { name: /Destination store/ }), {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("option", { name: label }));
}

function fill(quantity: string, reason: string) {
  fireEvent.change(screen.getByLabelText(/Quantity/), { target: { value: quantity } });
  fireEvent.change(screen.getByLabelText(/Reason/), { target: { value: reason } });
}

const submitButton = () => screen.getByRole("button", { name: /^Transfer$/ });

describe("StockTransferDialog", () => {
  beforeAll(async () => {
    Element.prototype.scrollIntoView = vi.fn();
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory } },
        interpolation: { escapeValue: false },
      });
    }
  });

  beforeEach(() => {
    vi.clearAllMocks();
    perms.allowed = true;
    svc.listInventoryLocations.mockResolvedValue([
      store(3, "BIN-A"),
      store(7, "BIN-B"),
      store(8, "BIN-OLD", 0),
    ]);
    svc.listInventoryLotStock.mockResolvedValue([]);
    svc.transferInventoryStock.mockResolvedValue([]);
  });

  it("prefills the source, offers only the other active stores and shows the available stock", async () => {
    renderDialog();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/ART-001 — Bearing 6204/)).toBeInTheDocument();
    expect(within(dialog).getByText(/MAIN — Main store/)).toBeInTheDocument();
    expect(within(dialog).getByTestId("transfer-available")).toHaveTextContent(
      "Available: 5 ea (8 in stock, of which 3 reserved and not transferable)",
    );
    fireEvent.keyDown(await screen.findByRole("combobox", { name: /Destination store/ }), {
      key: "Enter",
    });
    expect(await screen.findByRole("option", { name: /BIN-B/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /BIN-OLD/ })).toBeNull();
    expect(screen.queryByRole("option", { name: /BIN-A/ })).toBeNull();
  });

  it("caps the quantity at the available stock and sends nothing above it", async () => {
    renderDialog();
    await screen.findByRole("dialog");
    await chooseDestination(/BIN-B/);
    fill("6", "Workshop top-up");
    expect(screen.getByText("Maximum transferable: 5 ea")).toBeInTheDocument();
    fireEvent.click(submitButton());
    expect(
      await screen.findByText("The quantity exceeds the available stock (5 ea)."),
    ).toBeInTheDocument();
    expect(svc.transferInventoryStock).not.toHaveBeenCalled();
  });

  it("requires a reason of at least 5 characters", async () => {
    renderDialog();
    await screen.findByRole("dialog");
    await chooseDestination(/BIN-B/);
    fill("2", "  ab ");
    fireEvent.click(submitButton());
    expect(
      await screen.findByText("The reason must be at least 5 characters."),
    ).toBeInTheDocument();
    expect(svc.transferInventoryStock).not.toHaveBeenCalled();
  });

  it("states the move before saving, then sends the payload and refreshes", async () => {
    const { onTransferred, onOpenChange } = renderDialog();
    await screen.findByRole("dialog");
    await chooseDestination(/BIN-B/);
    fill("2", "  Workshop top-up ");
    expect(
      screen.getByText(/^Transfer 2 ea of Bearing 6204 from store .* to .*\?$/),
    ).toBeInTheDocument();
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(svc.transferInventoryStock).toHaveBeenCalledWith({
        article_id: 4,
        from_location_id: 3,
        to_location_id: 7,
        quantity: 2,
        notes: "Workshop top-up",
        lot_id: null,
      }),
    );
    await waitFor(() => expect(onTransferred).toHaveBeenCalled());
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(toast.pushAppToast).toHaveBeenCalledWith(
      expect.objectContaining({ variant: "success" }),
    );
  });

  it("shows the translated reason of a coded backend refusal and keeps the dialog open", async () => {
    svc.transferInventoryStock.mockRejectedValue({ code: "VALIDATION_FAILED" });
    const { onTransferred, onOpenChange } = renderDialog();
    await screen.findByRole("dialog");
    await chooseDestination(/BIN-B/);
    fill("2", "Workshop top-up");
    fireEvent.click(submitButton());
    expect(await screen.findByRole("alert")).toHaveTextContent("reason:VALIDATION_FAILED");
    expect(onTransferred).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("offers FEFO by default for a lot-tracked article, caps by the chosen lot and sends it", async () => {
    svc.listInventoryLotStock.mockResolvedValue([
      {
        lot_id: 11,
        lot_number: "L-11",
        serial_number: null,
        expiry_date: "2027-01-01",
        quantity: 3,
      },
    ]);
    renderDialog({ requires_batch_tracking: 1 });
    await screen.findByRole("dialog");
    await waitFor(() => expect(svc.listInventoryLotStock).toHaveBeenCalledWith(4, 3));
    expect(screen.getByRole("combobox", { name: /Lot or serial number/ })).toHaveTextContent(
      "Automatic: the lot that expires first (FEFO)",
    );
    await chooseDestination(/BIN-B/);
    fireEvent.keyDown(screen.getByRole("combobox", { name: /Lot or serial number/ }), {
      key: "Enter",
    });
    fireEvent.click(await screen.findByRole("option", { name: /L-11/ }));
    // The lot holds 3: the quantity is capped by the lot, not only by the available stock.
    fill("4", "Workshop top-up");
    fireEvent.click(submitButton());
    expect(
      await screen.findByText("The quantity exceeds the available stock (3 ea)."),
    ).toBeInTheDocument();
    fill("3", "Workshop top-up");
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(svc.transferInventoryStock).toHaveBeenCalledWith(
        expect.objectContaining({ quantity: 3, lot_id: 11 }),
      ),
    );
  });

  it("asks for whole units on a serial-tracked article", async () => {
    renderDialog({ requires_serial_tracking: 1 });
    await screen.findByRole("dialog");
    await chooseDestination(/BIN-B/);
    fill("1.5", "Workshop top-up");
    fireEvent.click(submitButton());
    expect(await screen.findByText("Enter a whole number of units.")).toBeInTheDocument();
    expect(svc.transferInventoryStock).not.toHaveBeenCalled();
  });
});

describe("StockTransferAction", () => {
  beforeEach(() => {
    perms.allowed = true;
    svc.listInventoryLocations.mockResolvedValue([]);
    svc.listInventoryLotStock.mockResolvedValue([]);
  });

  it("is shown with the inventory-manage right and opens the dialog", async () => {
    render(<StockTransferAction article={article()} balance={source} onTransferred={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Transfer/ }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("is hidden without the inventory-manage right", () => {
    perms.allowed = false;
    render(<StockTransferAction article={article()} balance={source} onTransferred={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /Transfer/ })).toBeNull();
  });
});

/**
 * L6-11: "Créer une demande d'achat" on the work order sheet. Offered with inv.procure on an open
 * work order with a parts shortage; hidden without the right, on a closed order, or with no
 * shortage. Same command, pre-filled from the work order's part line.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { WoPartsRequisitionEntry } from "@/components/wo/WoPartsRequisitionEntry";
import { WoShortageSection } from "@/components/wo/WoShortageSection";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, o?: { number?: string }) => (o?.number ? `${key}:${o.number}` : key),
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const access = vi.hoisted(() => ({ granted: true }));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: () => access.granted }),
}));

const inv = vi.hoisted(() => ({
  checkWoPartStockAvailability: vi.fn(),
  createProcurementRequisitionFromWoPart: vi.fn(),
}));
vi.mock("@/services/inventory-service", () => inv);

const readiness = {
  work_order_id: 42,
  work_order_code: "OT-0042",
  is_fully_available: false,
  shortages: [
    {
      article_id: 9,
      article_code: "ROU-6204",
      article_name: "Roulement 6204",
      requested_qty: 4,
      available_qty: 1,
      shortage_qty: 3,
      preferred_location_id: 5,
    },
  ],
};

describe("WoPartsRequisitionEntry", () => {
  beforeEach(() => {
    access.granted = true;
    inv.checkWoPartStockAvailability.mockReset().mockResolvedValue(readiness);
    inv.createProcurementRequisitionFromWoPart
      .mockReset()
      .mockResolvedValue({ id: 1, req_number: "DA-0007" });
  });

  it("is visible with the right and creates the request from the order's shortage", async () => {
    render(<WoPartsRequisitionEntry woId={42} statusCode="in_progress" />);
    const button = await screen.findByRole("button", {
      name: "planning.shortage.createRequisition",
    });
    fireEvent.click(button);
    await waitFor(() =>
      expect(inv.createProcurementRequisitionFromWoPart).toHaveBeenCalledWith(42, 9, 3, 5),
    );
    expect(
      await screen.findByText(/planning\.shortage\.requisitionCreatedNumber:DA-0007/),
    ).toBeTruthy();
  });

  it("is hidden without the right and does not read the stock", () => {
    access.granted = false;
    render(<WoPartsRequisitionEntry woId={42} statusCode="in_progress" />);
    expect(screen.queryByText("planning.shortage.title")).toBeNull();
    expect(inv.checkWoPartStockAvailability).not.toHaveBeenCalled();
  });

  it("is hidden on a closed work order", () => {
    render(<WoPartsRequisitionEntry woId={42} statusCode="closed" />);
    expect(screen.queryByText("planning.shortage.title")).toBeNull();
    expect(inv.checkWoPartStockAvailability).not.toHaveBeenCalled();
  });

  it("is hidden when no part is short", async () => {
    inv.checkWoPartStockAvailability.mockResolvedValue({
      ...readiness,
      is_fully_available: true,
      shortages: [],
    });
    render(<WoPartsRequisitionEntry woId={42} statusCode="ready" />);
    await waitFor(() => expect(inv.checkWoPartStockAvailability).toHaveBeenCalled());
    expect(screen.queryByText("planning.shortage.title")).toBeNull();
  });

  it("planning block: short parts stay visible without the right, but no request button", () => {
    access.granted = false;
    render(<WoShortageSection woId={42} readiness={readiness} />);
    expect(screen.getByText("ROU-6204")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "planning.shortage.createRequisition" }),
    ).toBeNull();
  });
});

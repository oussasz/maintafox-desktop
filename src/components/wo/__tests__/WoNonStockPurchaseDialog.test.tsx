/**
 * L3-04: buying a hors-stock part from the work order reuses a known hors-stock article when one
 * is chosen, and otherwise asks for the description and unit of a new one.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NonStockArticle, ReferenceValue } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

const UNIT = { id: 7, code: "U", label: "Unité", metadata_json: null } as unknown as ReferenceValue;
vi.mock("@/components/lookups/MeasureUnitPicker", () => ({
  useMeasureUnits: () => ({ units: [UNIT], loading: false, error: null }),
  MeasureUnitSelect: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: number | null;
    onChange: (v: number | null) => void;
  }) => (
    <select
      id={id}
      data-testid="unit"
      value={value == null ? "" : String(value)}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
    >
      <option value="">-</option>
      <option value="7">U</option>
    </select>
  ),
}));

const svc = vi.hoisted(() => ({
  searchNonStockArticles: vi.fn(),
  createWoNonStockPurchase: vi.fn(),
}));
vi.mock("@/services/wo-execution-service", () => svc);

import { WoNonStockPurchaseDialog } from "../WoNonStockPurchaseDialog";

const KNOWN: NonStockArticle = {
  id: 42,
  article_code: "HS-AB12-0001",
  article_name: "Joint torique 212",
  unit_value_id: 7,
  unit_symbol: "U",
  origin_work_order_id: 3,
  origin_work_order_code: "OT-AB12-0003",
  catalogue_review_state: "TO_REVIEW",
  duplicate_of_article_id: null,
  is_active: true,
};

function renderDialog(onCreated = vi.fn()) {
  render(<WoNonStockPurchaseDialog woId={9} open onOpenChange={vi.fn()} onCreated={onCreated} />);
  return onCreated;
}

describe("WoNonStockPurchaseDialog", () => {
  beforeEach(() => {
    svc.searchNonStockArticles.mockReset().mockResolvedValue([KNOWN]);
    svc.createWoNonStockPurchase.mockReset().mockResolvedValue({
      part: { id: 1, work_order_id: 9 },
      article: KNOWN,
      requisition: { id: 5, req_number: "DA-AB12-0001", status: "DRAFT" },
    });
  });

  it("asks for a description and a unit before creating a new hors-stock part", async () => {
    renderDialog();
    fireEvent.click(screen.getByText("nonStockPurchase.submit"));
    expect(await screen.findByRole("alert")).toHaveTextContent("nonStockPurchase.nameRequired");

    fireEvent.change(screen.getByLabelText("nonStockPurchase.name"), {
      target: { value: "Courroie XPZ 1250" },
    });
    fireEvent.click(screen.getByText("nonStockPurchase.submit"));
    expect(await screen.findByRole("alert")).toHaveTextContent("nonStockPurchase.unitRequired");

    fireEvent.change(screen.getByTestId("unit"), { target: { value: "7" } });
    fireEvent.click(screen.getByText("nonStockPurchase.submit"));
    await waitFor(() => expect(svc.createWoNonStockPurchase).toHaveBeenCalledTimes(1));
    expect(svc.createWoNonStockPurchase).toHaveBeenCalledWith(
      expect.objectContaining({
        work_order_id: 9,
        article_id: null,
        article_name: "Courroie XPZ 1250",
        unit_value_id: 7,
        quantity: 1,
      }),
    );
  });

  it("reuses a known hors-stock part instead of creating a twin", async () => {
    const onCreated = renderDialog();
    fireEvent.change(screen.getByLabelText("nonStockPurchase.search"), {
      target: { value: "joint" },
    });
    fireEvent.click(await screen.findByText(/HS-AB12-0001/));
    expect(screen.queryByLabelText("nonStockPurchase.name")).toBeNull();

    fireEvent.click(screen.getByText("nonStockPurchase.submit"));
    await waitFor(() => expect(svc.createWoNonStockPurchase).toHaveBeenCalledTimes(1));
    expect(svc.createWoNonStockPurchase).toHaveBeenCalledWith(
      expect.objectContaining({ article_id: 42, article_name: null, unit_value_id: null }),
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });
});

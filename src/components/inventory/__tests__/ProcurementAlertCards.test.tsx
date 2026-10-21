import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProcurementAlertCards } from "@/components/inventory/ProcurementAlertCards";
import enInventory from "@/i18n/locale-data/en/inventory.json";

const i18nMock = vi.hoisted(() => {
  const t = (key: string, opts?: Record<string, unknown>) => {
    const value = key
      .split(".")
      .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], {
        procurement: (globalThis as { __inv?: unknown }).__inv,
      });
    if (typeof value !== "string") return key;
    return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
  };
  return { t, i18n: { language: "en" } };
});
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

(globalThis as { __inv?: unknown }).__inv = enInventory.procurement;

describe("ProcurementAlertCards", () => {
  it("words coded alerts in the UI language and opens the related list", () => {
    const onNavigate = vi.fn();
    render(
      <ProcurementAlertCards
        onNavigate={onNavigate}
        alerts={[
          {
            kind: "CRITICAL_STOCK",
            severity: "high",
            entity_type: "ARTICLE",
            entity_id: 7,
            entity_code: "BRG-6204",
            params: {
              name: "Bearing",
              warehouse_code: "MAIN",
              available_qty: "0",
              threshold_qty: "5",
            },
          },
          {
            kind: "LATE_PO",
            severity: "high",
            entity_type: "PURCHASE_ORDER",
            entity_id: 9,
            entity_code: "PO-0009",
            params: { expected_date: "2026-01-05T00:00:00Z" },
          },
        ]}
      />,
    );
    expect(
      screen.getByText(/BRG-6204 Bearing — store MAIN: 0 available, threshold 5/),
    ).toBeInTheDocument();
    expect(screen.getByText(enInventory.procurement.alerts.kinds.LATE_PO)).toBeInTheDocument();
    fireEvent.click(screen.getByText(/PO-0009 — expected/));
    expect(onNavigate).toHaveBeenCalledWith(
      "purchase-orders",
      expect.objectContaining({ entity_id: 9 }),
    );
  });

  it("renders nothing without alerts", () => {
    const { container } = render(<ProcurementAlertCards alerts={[]} onNavigate={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });
});

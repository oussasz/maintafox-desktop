import { act, fireEvent, render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { WoRetiredEquipmentReviewPanel } from "@/components/wo/WoRetiredEquipmentReviewPanel";
import enOt from "@/i18n/locale-data/en/ot.json";
import frOt from "@/i18n/locale-data/fr/ot.json";

const mocks = vi.hoisted(() => ({
  listRetiredEquipmentReviews: vi.fn(),
  openWo: vi.fn(),
}));

vi.mock("@/services/retired-equipment-review-service", () => ({
  listRetiredEquipmentReviews: mocks.listRetiredEquipmentReviews,
}));
vi.mock("@/stores/wo-store", () => ({
  useWoStore: (sel: (s: { openWo: (id: number) => Promise<void> }) => unknown) =>
    sel({ openWo: mocks.openWo }),
}));

function review(overrides: Record<string, unknown> = {}) {
  return {
    id: 1,
    review_key: "wo|sid-1",
    kind: "work_order",
    record_id: 41,
    record_code: "OT-0041",
    record_title: "Pump seal",
    record_status: "planning",
    equipment_id: 1,
    equipment_code: "EQ-01",
    equipment_name: "Pump",
    equipment_status: "DECOMMISSIONED",
    status: "open",
    opened_at: "2026-10-26T08:00:00Z",
    closed_at: null,
    closure_kind: null,
    closed_by: null,
    closure_note: null,
    converted_to_wo_id: null,
    converted_to_wo_code: null,
    disposition_code: null,
    ...overrides,
  };
}

function renderPanel() {
  return render(
    <MemoryRouter>
      <WoRetiredEquipmentReviewPanel />
    </MemoryRouter>,
  );
}

describe("WoRetiredEquipmentReviewPanel", () => {
  beforeAll(async () => {
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["ot"],
        defaultNS: "ot",
        resources: { en: { ot: enOt } },
        interpolation: { escapeValue: false },
      });
    } else {
      i18next.addResourceBundle("en", "ot", enOt, true, true);
    }
    await i18next.changeLanguage("en");
  });

  beforeEach(() => {
    mocks.listRetiredEquipmentReviews.mockReset();
    mocks.openWo.mockReset();
  });

  it("lists an open work order and an open request with no dismiss action, and opens them", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValue({
      items: [
        review(),
        review({
          id: 2,
          review_key: "di|sid-2",
          kind: "intervention_request",
          record_id: 7,
          record_code: "DI-0007",
          record_title: "Vibration",
        }),
      ],
    });
    renderPanel();

    expect(await screen.findByText("Retired equipment — to review")).toBeInTheDocument();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledWith(true);
    expect(screen.getByText("OT-0041")).toBeInTheDocument();
    expect(screen.getByText("DI-0007")).toBeInTheDocument();
    // Settled only with the existing actions: nothing to dismiss, accept or close here.
    expect(
      screen.queryByRole("button", { name: /dismiss|accept|ignore|close/i }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(mocks.openWo).toHaveBeenCalledWith(41);
    expect(screen.getByRole("link", { name: "Open" })).toHaveAttribute(
      "href",
      "/requests?openDi=7",
    );
  });

  it("keeps who, when and how of a closed item in the history", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValue({
      items: [
        review({
          status: "closed",
          closed_at: "2026-10-26T10:00:00Z",
          closure_kind: "wo_cancelled",
          closed_by: "Amine",
          closure_note: "Equipment scrapped",
        }),
        review({
          id: 3,
          kind: "intervention_request",
          record_code: "DI-0009",
          status: "closed",
          closed_at: "2026-10-26T09:00:00Z",
          closure_kind: "di_converted",
          converted_to_wo_code: "OT-0050",
          closed_by: null,
        }),
      ],
    });
    renderPanel();

    expect(await screen.findByText(/Nothing left to review/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "History (2)" }));
    expect(
      screen.getByText("Closed by the cancellation of work order OT-0041"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Amine, .*Reason: Equipment scrapped/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "Closed by the conversion of request DI-0009 into work order OT-0050 (the work order has its own case)",
      ),
    ).toBeInTheDocument();
  });

  it("shows nothing when there is nothing to review", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValue({ items: [] });
    const { container } = renderPanel();
    await act(async () => {
      await Promise.resolve();
    });
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });

  it("says so when the list cannot be read", async () => {
    mocks.listRetiredEquipmentReviews.mockRejectedValue(new Error("ipc"));
    renderPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load the work orders and requests on retired equipment.",
    );
  });

  it("has every text in French and English", () => {
    const keys = (o: Record<string, unknown>, prefix = ""): string[] =>
      Object.entries(o).flatMap(([k, v]) =>
        typeof v === "object" && v !== null
          ? keys(v as Record<string, unknown>, `${prefix}${k}.`)
          : [`${prefix}${k}`],
      );
    expect(keys(frOt.retiredEquipment).sort()).toEqual(keys(enOt.retiredEquipment).sort());
  });
});

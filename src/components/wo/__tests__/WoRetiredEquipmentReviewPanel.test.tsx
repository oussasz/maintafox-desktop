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
  store: { items: [] as unknown[] },
}));

vi.mock("@/services/retired-equipment-review-service", () => ({
  listRetiredEquipmentReviews: mocks.listRetiredEquipmentReviews,
}));
vi.mock("@/stores/wo-store", () => ({
  useWoStore: (sel: (s: { openWo: (id: number) => Promise<void>; items: unknown[] }) => unknown) =>
    sel({ openWo: mocks.openWo, items: mocks.store.items }),
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

const CLOSED = [
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
  }),
];

function renderPanel() {
  return render(
    <MemoryRouter>
      <WoRetiredEquipmentReviewPanel />
    </MemoryRouter>,
  );
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
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
    mocks.store.items = [];
  });

  it("lists the open items only, with no dismiss action, and opens them", async () => {
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
      closed_count: 0,
    });
    renderPanel();

    expect(await screen.findByText("Retired equipment — to review")).toBeInTheDocument();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledTimes(1);
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledWith(false, false);
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

  it("reads the history only when it is opened, and keeps who, when and how", async () => {
    mocks.listRetiredEquipmentReviews.mockImplementation(async (includeClosed: boolean) => ({
      items: [review(), ...(includeClosed ? CLOSED : [])],
      closed_count: 2,
    }));
    renderPanel();

    const toggle = await screen.findByRole("button", { name: "History (2)" });
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/Closed by/)).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(
      await screen.findByText("Closed by the cancellation of work order OT-0041"),
    ).toBeInTheDocument();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenLastCalledWith(true, false);
    expect(screen.getByText(/Amine, .*Reason: Equipment scrapped/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "Closed by the conversion of request DI-0009 into work order OT-0050 (the work order has its own case)",
      ),
    ).toBeInTheDocument();
  });

  it("hides the card when nothing is open, keeping a link to the history", async () => {
    mocks.listRetiredEquipmentReviews.mockImplementation(async (includeClosed: boolean) => ({
      items: includeClosed ? CLOSED : [],
      closed_count: 2,
    }));
    renderPanel();

    const link = await screen.findByRole("button", { name: "Retired equipment: history (2)" });
    expect(screen.queryByTestId("retired-equipment-review")).not.toBeInTheDocument();
    fireEvent.click(link);
    expect(
      await screen.findByText("Closed by the cancellation of work order OT-0041"),
    ).toBeInTheDocument();
  });

  it("reads the open items again when the work-order list reloads", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValueOnce({ items: [review()], closed_count: 0 });
    mocks.listRetiredEquipmentReviews.mockResolvedValue({ items: [], closed_count: 1 });
    const view = renderPanel();
    expect(await screen.findByText("OT-0041")).toBeInTheDocument();

    // The work order was cancelled from the page: the list reloads.
    mocks.store.items = [{ id: 41 }];
    view.rerender(
      <MemoryRouter>
        <WoRetiredEquipmentReviewPanel />
      </MemoryRouter>,
    );
    await flush();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("OT-0041")).not.toBeInTheDocument();
  });

  it("keeps an open history open when the work-order list reloads, and reads it again", async () => {
    mocks.listRetiredEquipmentReviews.mockImplementation(async (includeClosed: boolean) => ({
      items: [review(), ...(includeClosed ? CLOSED : [])],
      closed_count: 2,
    }));
    const view = renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "History (2)" }));
    expect(
      await screen.findByText("Closed by the cancellation of work order OT-0041"),
    ).toBeInTheDocument();

    mocks.store.items = [{ id: 99 }];
    view.rerender(
      <MemoryRouter>
        <WoRetiredEquipmentReviewPanel />
      </MemoryRouter>,
    );
    await flush();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenLastCalledWith(true, false);
    expect(
      screen.getByText("Closed by the cancellation of work order OT-0041"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "History (2)" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("shows nothing when there is nothing to review", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValue({ items: [], closed_count: 0 });
    const { container } = renderPanel();
    await flush();
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

  it("on the Requests page lists the requests only and opens them in place (AU-112)", async () => {
    mocks.listRetiredEquipmentReviews.mockResolvedValue({
      items: [
        review({
          id: 2,
          review_key: "di|sid-2",
          kind: "intervention_request",
          record_id: 7,
          record_code: "DI-0007",
          record_title: "Vibration",
        }),
      ],
      closed_count: 0,
    });
    const onOpenRequest = vi.fn();
    const { rerender } = render(
      <MemoryRouter>
        <WoRetiredEquipmentReviewPanel
          requestsOnly
          onOpenRequest={onOpenRequest}
          reloadSignal={1}
        />
      </MemoryRouter>,
    );

    expect(await screen.findByText("DI-0007")).toBeInTheDocument();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledWith(false, true);
    expect(screen.queryByRole("link", { name: "Open" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(onOpenRequest).toHaveBeenCalledWith(7);

    // The work-order list does not drive it there: the Requests page's own list does.
    mocks.store.items = [{}];
    rerender(
      <MemoryRouter>
        <WoRetiredEquipmentReviewPanel
          requestsOnly
          onOpenRequest={onOpenRequest}
          reloadSignal={1}
        />
      </MemoryRouter>,
    );
    await flush();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledTimes(1);
    rerender(
      <MemoryRouter>
        <WoRetiredEquipmentReviewPanel
          requestsOnly
          onOpenRequest={onOpenRequest}
          reloadSignal={2}
        />
      </MemoryRouter>,
    );
    await flush();
    expect(mocks.listRetiredEquipmentReviews).toHaveBeenCalledTimes(2);
  });
});

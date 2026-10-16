/**
 * Home role panels: real figures from the one panel call, a failed load shows an error with a
 * retry (never a 0), sections of modules the user may not open are absent, and every figure or
 * record opens the exact screen behind it.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { HomeMyWork, HomeSupervision } from "@shared/ipc-types";

import { HomeMyWorkPanel } from "../home/HomeMyWorkPanel";
import { HomeSupervisionPanel } from "../home/HomeSupervisionPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "count" in opts ? `${key}:${String(opts["count"])}` : key,
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  getHomeMyWork: vi.fn(),
  getHomeSupervision: vi.fn(),
  getHomePlanning: vi.fn(),
  HOME_PERIODS: [7, 30],
}));
vi.mock("@/services/home-service", () => svc);

const queues = vi.hoisted(() => ({ visible: ["all", "mine"] as string[] }));
vi.mock("@/components/di/DiQueueBar", () => ({ useVisibleDiQueues: () => queues.visible }));
vi.mock("@/components/common/urgency/use-urgency-scale", () => ({
  useUrgencyScale: () => ({ levels: [{ code: "emergency" }], labelOf: (c: string) => `U:${c}` }),
  urgencyTone: () => "neutral",
}));
vi.mock("@/components/wo/use-wo-type-labels", () => ({
  useWoTypeLabels: () => ({ types: [], language: "en" }),
  woNatureLabel: () => null,
}));

function Where() {
  const loc = useLocation();
  return <p data-testid="where">{`${loc.pathname}${loc.search}`}</p>;
}

function renderAt(node: React.ReactNode) {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={node} />
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
}

const myWork: HomeMyWork = {
  as_of: "2026-10-16T08:00:00Z",
  work_orders: {
    open: 3,
    overdue: 1,
    today: 1,
    by_urgency: [{ urgency_code: "emergency", count: 1 }],
    items: [
      {
        id: 44,
        code: "OT-44",
        title: "Seal",
        status_code: "in_progress",
        urgency_code: "emergency",
        planned_start: null,
        planned_end: "2026-10-15T12:00:00Z",
        asset_label: "P-101",
        overdue: true,
      },
    ],
  },
  requests: { open: 2, awaiting_my_answer: 1 },
  rounds: { personnel_linked: false, overdue: 0, today: 0, next_7_days: 0, items: [] },
  pm: null,
  notifications: { unread: 5, to_acknowledge: 2 },
};

describe("HomeMyWorkPanel", () => {
  beforeEach(() => {
    svc.getHomeMyWork.mockReset();
    queues.visible = ["all", "mine"];
  });

  it("shows the figures and opens the record behind a row", async () => {
    svc.getHomeMyWork.mockResolvedValueOnce(myWork);
    renderAt(<HomeMyWorkPanel />);
    expect(await screen.findByText("OT-44 — Seal")).toBeTruthy();
    expect(screen.getByText("home.myWork.rounds.notLinked")).toBeTruthy();
    expect(screen.queryByText("home.myWork.pm.title")).toBeNull();
    fireEvent.click(screen.getByText("OT-44 — Seal"));
    expect(screen.getByTestId("where").textContent).toBe("/work-orders?openWo=44");
  });

  it("opens my requests queue", async () => {
    svc.getHomeMyWork.mockResolvedValueOnce(myWork);
    renderAt(<HomeMyWorkPanel />);
    const tile = await screen.findByRole("button", { name: /home\.myWork\.di\.awaiting: 1/ });
    fireEvent.click(tile);
    expect(screen.getByTestId("where").textContent).toBe("/requests?queue=mine");
  });

  it("shows a load failure with a retry, never a zero", async () => {
    svc.getHomeMyWork.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "boom" });
    svc.getHomeMyWork.mockResolvedValueOnce(myWork);
    renderAt(<HomeMyWorkPanel />);
    const retry = await screen.findByRole("button", { name: "ds.state.retry" });
    expect(screen.queryByText("OT-44 — Seal")).toBeNull();
    fireEvent.click(retry);
    expect(await screen.findByText("OT-44 — Seal")).toBeTruthy();
    expect(svc.getHomeMyWork).toHaveBeenCalledTimes(2);
  });
});

const supervision: HomeSupervision = {
  as_of: "2026-10-16T08:00:00Z",
  requests: null,
  work_orders: null,
  pm: null,
  stock: {
    lines_to_reorder: 1,
    articles_to_reorder: 1,
    lines_out_of_stock: 1,
    items: [
      {
        article_id: 7,
        article_code: "BRG",
        article_name: "Bearing",
        warehouse_code: "MAIN",
        available_qty: 0,
        reorder_point: 4,
        min_stock: 2,
        trigger_type: "reorder_point",
      },
    ],
  },
  safety: null,
  assets: {
    critical_down: 1,
    criticality_label: "A",
    down_status_labels: ["Out of service"],
    items: [{ id: 9, asset_code: "P-101", name: "Pump", status_label: "Out of service" }],
  },
};

describe("HomeSupervisionPanel", () => {
  beforeEach(() => {
    svc.getHomeSupervision.mockReset();
  });

  it("loads the default period and reloads on a new one", async () => {
    svc.getHomeSupervision.mockResolvedValue(supervision);
    renderAt(<HomeSupervisionPanel />);
    expect(await screen.findByText("BRG — Bearing")).toBeTruthy();
    expect(svc.getHomeSupervision).toHaveBeenLastCalledWith(30);
    fireEvent.click(screen.getByRole("radio", { name: "home.period.days:7" }));
    await waitFor(() => expect(svc.getHomeSupervision).toHaveBeenLastCalledWith(7));
  });

  it("opens the article and the asset behind the rows", async () => {
    svc.getHomeSupervision.mockResolvedValue(supervision);
    renderAt(<HomeSupervisionPanel />);
    fireEvent.click(await screen.findByText("P-101 — Pump"));
    expect(screen.getByTestId("where").textContent).toBe("/equipment?assetId=9");
  });

  it("says so when no module of the panel is open to the user", async () => {
    svc.getHomeSupervision.mockResolvedValue({
      ...supervision,
      stock: null,
      assets: null,
    });
    renderAt(<HomeSupervisionPanel />);
    expect(await screen.findByText("home.supervision.noModule")).toBeTruthy();
  });
});

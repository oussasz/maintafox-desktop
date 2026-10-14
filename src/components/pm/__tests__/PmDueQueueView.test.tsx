import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { PmDueQueueView } from "@/components/pm/PmDueQueueView";
import type { PmOccurrenceRow } from "@/services/pm-service";

vi.mock("react-i18next", async () => (await import("./pm-test-i18n")).pmI18nMock);
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const navigate = vi.hoisted(() => vi.fn());
vi.mock("react-router-dom", async (orig) => ({
  ...(await orig<Record<string, unknown>>()),
  useNavigate: () => navigate,
}));

function occ(overrides: Partial<PmOccurrenceRow> = {}): PmOccurrenceRow {
  return {
    id: 1,
    pm_plan_id: 3,
    plan_version_id: 4,
    due_basis: "fixed:2099-01-10T00:00:00Z",
    due_at: "2099-01-10T00:00:00Z",
    due_meter_value: null,
    generated_at: "2026-01-01T00:00:00Z",
    status: "scheduled",
    linked_work_order_id: 88,
    linked_work_order_code: "OT-2026-0144",
    deferral_reason: null,
    missed_reason: null,
    row_version: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    plan_code: "PM-PUMP",
    plan_title: "Graissage pompe",
    strategy_type: "fixed",
    asset_code: "P-101",
    asset_name: "Pompe de reprise",
    ...overrides,
  };
}

function renderQueue(rows: PmOccurrenceRow[], handlers: Partial<Record<string, () => void>> = {}) {
  return render(
    <MemoryRouter>
      <PmDueQueueView
        occurrences={rows}
        loading={false}
        onOpenPlan={handlers["open"] ?? vi.fn()}
        onTransition={handlers["transition"] ?? vi.fn()}
        onExecute={handlers["execute"] ?? vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe("PmDueQueueView", () => {
  it("shows the asset, a translated status and the OT as a link", () => {
    renderQueue([occ()]);
    expect(screen.getByText("P-101")).toBeInTheDocument();
    expect(screen.getByText("Pompe de reprise")).toBeInTheDocument();
    expect(screen.getByText("Scheduled")).toBeInTheDocument();
    const link = screen.getByRole("link");
    fireEvent.click(link);
    expect(navigate).toHaveBeenCalledWith("/work-orders?openWo=88");
    expect(screen.queryByText("scheduled")).not.toBeInTheDocument();
  });

  it("offers Log execution only for an in-progress occurrence", () => {
    const execute = vi.fn();
    renderQueue(
      [
        occ({ id: 1, status: "scheduled", plan_code: "PM-A" }),
        occ({ id: 2, status: "in_progress", plan_code: "PM-B" }),
      ],
      { execute },
    );
    const buttons = screen.getAllByRole("button", { name: "Log execution" });
    expect(buttons).toHaveLength(1);
    fireEvent.click(buttons[0] as HTMLElement);
    expect(execute).toHaveBeenCalledWith(expect.objectContaining({ id: 2 }));
    expect(screen.getAllByRole("button", { name: "Change status" })).toHaveLength(2);
  });

  it("flags an open occurrence past its due date as overdue", () => {
    renderQueue([occ({ due_at: "2020-01-01T00:00:00Z" })]);
    const row = screen.getByText("PM-PUMP").closest("tr") as HTMLElement;
    expect(within(row).getByText("Overdue")).toBeInTheDocument();
  });

  it("shows a meter-due occurrence by its reading", () => {
    renderQueue([occ({ due_at: null, due_meter_value: 1250, linked_work_order_code: null })]);
    expect(screen.getByText(/At 1[\s,.]?250 on the meter/)).toBeInTheDocument();
  });
});

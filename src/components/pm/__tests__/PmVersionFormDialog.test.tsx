import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PmVersionFormDialog } from "@/components/pm/PmVersionFormDialog";
import type { PmPlanRow } from "@/services/pm-service";

vi.mock("react-i18next", async () => (await import("./pm-test-i18n")).pmI18nMock);
vi.mock("@/services/inventory-service", () => ({
  listInventoryArticles: vi.fn().mockResolvedValue([
    {
      id: 4,
      article_code: "FLT-10",
      article_name: "Filtre à huile",
      unit_label: "pce",
      available_qty: 5,
      is_active: 1,
    },
  ]),
}));
vi.mock("@/services/lookup-service", () => ({
  getLookupValues: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/pm-service", () => ({
  listPmAssetMeters: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn().mockResolvedValue([
    { id: 1, code: "MECA", label: "Mécanicien", is_active: true },
    { id: 2, code: "ELEC", label: "Électricien", is_active: true },
  ]),
}));

const TS = "2026-10-01T10:00:00Z";

const plan: PmPlanRow = {
  id: 1,
  code: "PM-PUMP",
  title: "Graissage pompe",
  description: null,
  asset_scope_type: "equipment",
  asset_scope_id: 10,
  strategy_type: "fixed",
  criticality_value_id: null,
  criticality_code: null,
  criticality_label: null,
  assigned_team_id: null,
  assigned_team_code: null,
  assigned_team_name: null,
  assigned_team_label: null,
  requires_shutdown: 0,
  requires_permit: 0,
  is_active: 1,
  lifecycle_status: "approved",
  current_version_id: null,
  row_version: 1,
  created_at: TS,
  updated_at: TS,
};

describe("PmVersionFormDialog", () => {
  it("saves structured parts (article + quantity) and skills from the catalogue", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <PmVersionFormDialog
        open
        mode="create"
        plan={plan}
        onClose={vi.fn()}
        onCreate={onCreate}
        onUpdate={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Add an article"), { target: { value: "FLT" } });
    fireEvent.click(await screen.findByRole("option", { name: /FLT-10/ }));
    const qty = screen.getByLabelText("Quantity of FLT-10");
    fireEvent.change(qty, { target: { value: "2" } });
    fireEvent.click(await screen.findByRole("checkbox", { name: "Mécanicien" }));
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(onCreate).toHaveBeenCalled());
    const payload = onCreate.mock.calls[0]?.[1] as Record<string, unknown>;
    expect(payload["required_parts_json"]).toBe('[{"article_code":"FLT-10","quantity":2}]');
    expect(payload["required_skills_json"]).toBe('["MECA"]');
    expect(JSON.parse(String(payload["trigger_definition_json"]))).toEqual({
      interval_unit: "day",
      interval_value: 30,
    });
  });

  it("asks for the asset's meter when the calendar plan also counts on usage", async () => {
    const onCreate = vi.fn();
    render(
      <PmVersionFormDialog
        open
        mode="create"
        plan={plan}
        onClose={vi.fn()}
        onCreate={onCreate}
        onUpdate={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Or on the meter, whichever comes first" }),
    );
    expect(
      await screen.findByText(
        "This equipment has no active meter. Add one on the equipment to plan by usage.",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Choose the meter of the equipment.",
    );
    expect(onCreate).not.toHaveBeenCalled();
  });
});

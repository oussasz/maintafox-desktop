/**
 * Inspection module screens (Phase 6.3): KPI tiles open the list behind their figure, the
 * anomaly list links the request created from a finding, the checklist editor names the row
 * and the reason before saving, and the schedule editor checks its fields.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enInspections from "@/i18n/locale-data/en/inspections.json";
import { InspectionsPage } from "@/pages/InspectionsPage";
import type { InspectionAnomalyRow, InspectionTemplate } from "@shared/ipc-types";

import { InspectionAnomaliesPanel } from "../InspectionAnomaliesPanel";
import { followUpBlocker } from "../InspectionFollowUpDialog";
import { InspectionScheduleDialog } from "../InspectionSchedulesPanel";
import { TemplateBuilderDialog } from "../InspectionTemplatesPanel";

const TOP_LEVEL = new Set(Object.keys(enInspections));

// Inspection keys resolve against the real English catalogue (a missing key fails the test);
// keys of shared components (common namespace) come back as the key.
function translate(key: string, opts?: Record<string, unknown>): string {
  if (key.includes(":") || !TOP_LEVEL.has(key.split(".")[0] ?? "")) {
    return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
  }
  const count = typeof opts?.["count"] === "number" ? opts["count"] : undefined;
  const lookup = (k: string) =>
    k
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        enInspections,
      );
  let value = lookup(key);
  if (value === undefined && count !== undefined) {
    value = lookup(`${key}_${count === 1 ? "one" : "other"}`);
  }
  if (typeof value !== "string") {
    if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
    throw new Error(`missing inspections i18n key: ${key}`);
  }
  return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate, i18n: { language: "en" } }),
}));

const svc = vi.hoisted(() => ({
  getInspectionKpis: vi.fn(),
  listInspectionRoundPage: vi.fn(),
  listInspectionAnomalyRows: vi.fn(),
  listInspectionSchedules: vi.fn(),
  listInspectionTemplates: vi.fn(),
  listInspectionTemplateVersions: vi.fn(),
  listInspectionCheckpoints: vi.fn(),
  listInspectionReliabilitySignals: vi.fn(),
  createInspectionTemplate: vi.fn(),
  publishInspectionTemplateVersion: vi.fn(),
  saveInspectionSchedule: vi.fn(),
  generateInspectionRounds: vi.fn(),
  deferInspectionAnomaly: vi.fn(),
  getInspectionAnomalyRow: vi.fn(),
  routeInspectionAnomalyToDi: vi.fn(),
  routeInspectionAnomalyToWo: vi.fn(),
  scheduleInspectionRound: vi.fn(),
  startInspectionRound: vi.fn(),
  refreshInspectionReliabilitySignals: vi.fn(),
}));
vi.mock("@/services/inspection-service", () => svc);
vi.mock("@/services/personnel-service", () => ({
  listActiveTeams: vi.fn().mockResolvedValue([]),
  listPersonnel: vi.fn().mockResolvedValue({ items: [], total: 0 }),
}));
const lifecycle = vi.hoisted(() => ({ listAssetMeters: vi.fn() }));
vi.mock("@/services/asset-lifecycle-service", () => lifecycle);
vi.mock("@/services/asset-service", () => ({ getAssetByIdSilent: vi.fn() }));
vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({
    onChange,
  }: {
    onChange: (a: { id: number; asset_code: string; asset_name: string } | null) => void;
  }) => (
    <button
      type="button"
      data-testid="asset-picker"
      onClick={() => onChange({ id: 10, asset_code: "P-101", asset_name: "Feed pump" })}
    >
      pick asset
    </button>
  ),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const kpis = {
  period_start: "2026-09-15T00:00:00Z",
  period_end: "2026-10-15T00:00:00Z",
  due_count: 4,
  done_count: 2,
  done_on_time_count: 1,
  missed_count: 1,
  overdue_count: 2,
  compliance_pct: 25,
  open_anomaly_count: 3,
};

function anomalyRow(partial: Partial<InspectionAnomalyRow> = {}): InspectionAnomalyRow {
  return {
    anomaly: {
      id: 7,
      round_id: 1,
      result_id: 8,
      anomaly_type: "threshold_breach",
      severity: 4,
      description: "x",
      linked_di_id: null,
      linked_work_order_id: null,
      requires_permit_review: false,
      resolution_status: "open",
      routing_decision: null,
      entity_sync_id: "a7",
      row_version: 1,
    },
    round_status: "completed_with_findings",
    round_scheduled_at: "2026-10-15T06:00:00Z",
    round_completed_at: null,
    template_code: "RND-1",
    template_name: "Pump house",
    checkpoint_id: 5,
    checkpoint_code: "PRESS",
    checkpoint_description: "Pressure",
    check_type: "numeric",
    measurement_unit: "bar",
    normal_min: null,
    normal_max: 7.5,
    warning_min: null,
    warning_max: 9,
    asset_id: 10,
    asset_code: "P-101",
    asset_name: "Feed pump",
    asset_has_location: true,
    result_status: "warning",
    numeric_value: 8.4,
    text_value: null,
    result_comment: "Vibrating",
    recorded_at: "2026-10-15T06:10:00Z",
    violation_direction: "above",
    violation_limit: 7.5,
    linked_di_code: null,
    linked_di_title: null,
    linked_di_status: null,
    linked_wo_code: null,
    linked_wo_title: null,
    ...partial,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  lifecycle.listAssetMeters.mockResolvedValue([]);
  svc.getInspectionKpis.mockResolvedValue(kpis);
  svc.listInspectionRoundPage.mockResolvedValue({ items: [], total: 0 });
  svc.listInspectionAnomalyRows.mockResolvedValue({ items: [], total: 0 });
  svc.listInspectionSchedules.mockResolvedValue([]);
  svc.listInspectionTemplates.mockResolvedValue([]);
  svc.listInspectionReliabilitySignals.mockResolvedValue([]);
});

describe("InspectionsPage", () => {
  it("shows the figures and opens the list behind each tile", async () => {
    render(
      <MemoryRouter>
        <InspectionsPage />
      </MemoryRouter>,
    );
    const tiles = await screen.findByTestId("inspection-kpis");
    await waitFor(() =>
      expect(within(tiles).getByRole("button", { name: /^Overdue: 2/ })).toBeInTheDocument(),
    );
    expect(within(tiles).getByRole("button", { name: /^Compliance: 25 %/ })).toBeInTheDocument();
    expect(svc.getInspectionKpis).toHaveBeenCalledWith({ period_days: 30 });
    // Default list: rounds still to do.
    await waitFor(() =>
      expect(svc.listInspectionRoundPage).toHaveBeenCalledWith(
        expect.objectContaining({ statuses: ["scheduled", "released", "in_progress"] }),
      ),
    );

    fireEvent.click(within(tiles).getByRole("button", { name: /^Overdue: 2/ }));
    await waitFor(() =>
      expect(svc.listInspectionRoundPage).toHaveBeenLastCalledWith(
        expect.objectContaining({ statuses: null, overdue_only: true }),
      ),
    );

    fireEvent.click(within(tiles).getByRole("button", { name: /^Rounds done: 2/ }));
    await waitFor(() =>
      expect(svc.listInspectionRoundPage).toHaveBeenLastCalledWith(
        expect.objectContaining({
          statuses: ["completed", "completed_with_findings", "reviewed"],
          due_from: kpis.period_start,
          due_to: kpis.period_end,
        }),
      ),
    );

    fireEvent.click(within(tiles).getByRole("button", { name: /^Open anomalies: 3/ }));
    await waitFor(() =>
      expect(svc.listInspectionAnomalyRows).toHaveBeenCalledWith(
        expect.objectContaining({ statuses: ["open"] }),
      ),
    );
  });

  it("shows an error state with a retry when the figures fail", async () => {
    svc.getInspectionKpis.mockRejectedValueOnce(new Error("db locked"));
    render(
      <MemoryRouter>
        <InspectionsPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("The figures could not be loaded")).toBeInTheDocument();
    svc.getInspectionKpis.mockResolvedValue(kpis);
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(await screen.findByRole("button", { name: /^Overdue: 2/ })).toBeInTheDocument();
  });
});

describe("InspectionAnomaliesPanel", () => {
  it("links the request created from a finding and offers decisions on open ones", async () => {
    svc.listInspectionAnomalyRows.mockResolvedValue({
      items: [
        anomalyRow(),
        anomalyRow({
          anomaly: {
            ...anomalyRow().anomaly,
            id: 8,
            linked_di_id: 55,
            resolution_status: "triaged",
          },
          linked_di_code: "DI-0042",
          linked_di_title: "Pressure high",
        }),
      ],
      total: 2,
    });
    const onOpenRound = vi.fn();
    render(
      <MemoryRouter>
        <InspectionAnomaliesPanel
          status={null}
          refreshKey={0}
          onChanged={vi.fn()}
          onOpenRound={onOpenRound}
        />
      </MemoryRouter>,
    );
    const list = await screen.findByTestId("inspection-anomaly-list");
    await waitFor(() => expect(within(list).getByText("linkedEntity.di")).toBeInTheDocument());
    expect(within(list).getAllByRole("button", { name: "Create a request" })).toHaveLength(1);
    expect(within(list).getByText("To decide")).toBeInTheDocument();
    expect(within(list).getByText("Followed up")).toBeInTheDocument();

    fireEvent.click(within(list).getAllByRole("button", { name: /Pump house/ })[0] as HTMLElement);
    expect(onOpenRound).toHaveBeenCalledWith(1);

    svc.deferInspectionAnomaly.mockResolvedValue({
      ...anomalyRow().anomaly,
      resolution_status: "deferred",
    });
    fireEvent.click(within(list).getByRole("button", { name: "Defer" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Defer" }));
    await waitFor(() =>
      expect(svc.deferInspectionAnomaly).toHaveBeenCalledWith({
        anomaly_id: 7,
        expected_row_version: 1,
      }),
    );
  });

  it("explains an empty list", async () => {
    render(
      <MemoryRouter>
        <InspectionAnomaliesPanel
          status="open"
          refreshKey={0}
          onChanged={vi.fn()}
          onOpenRound={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(await screen.findByText("No anomaly in this list.")).toBeInTheDocument();
  });

  it("says why a request cannot be created", () => {
    expect(followUpBlocker(anomalyRow(), "di")).toBeNull();
    expect(followUpBlocker(anomalyRow({ asset_id: null, asset_code: null }), "di")).toBe(
      "followUp.blocked.noEquipment",
    );
    expect(followUpBlocker(anomalyRow({ asset_id: null }), "wo")).toBe(
      "followUp.blocked.noEquipment",
    );
    expect(followUpBlocker(anomalyRow({ asset_has_location: false }), "wo")).toBeNull();
    expect(followUpBlocker(anomalyRow({ asset_has_location: false }), "di")).toBe(
      "followUp.blocked.noLocation",
    );
    expect(
      followUpBlocker(anomalyRow({ anomaly: { ...anomalyRow().anomaly, linked_di_id: 3 } }), "wo"),
    ).toBe("followUp.blocked.alreadyRouted");
  });
});

describe("TemplateBuilderDialog", () => {
  it("names the invalid rows and the reason instead of saving silently", async () => {
    render(<TemplateBuilderDialog target="new" onClose={vi.fn()} onSaved={vi.fn()} />);
    await screen.findByTestId("asset-picker");
    fireEvent.change(document.getElementById("tpl-code") as HTMLElement, {
      target: { value: "RND-1" },
    });
    fireEvent.change(document.getElementById("tpl-name") as HTMLElement, {
      target: { value: "Pump house" },
    });
    // Row 1 has no code.
    fireEvent.click(screen.getByRole("button", { name: "Save checklist" }));
    const summary = await screen.findByTestId("template-row-errors");
    expect(summary).toHaveTextContent("1 checkpoint has a problem. Nothing was saved.");
    expect(summary).toHaveTextContent("Checkpoint 1 (no code) : The code is required.");
    expect(svc.createInspectionTemplate).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Code of checkpoint 1"), { target: { value: "PRESS" } });
    await waitFor(() =>
      expect(screen.queryByTestId("template-row-errors")).not.toBeInTheDocument(),
    );
    svc.createInspectionTemplate.mockResolvedValue([{}, {}, []]);
    fireEvent.click(screen.getByRole("button", { name: "Save checklist" }));
    await waitFor(() =>
      expect(svc.createInspectionTemplate).toHaveBeenCalledWith(
        expect.objectContaining({
          code: "RND-1",
          checkpoints: [expect.objectContaining({ checkpoint_code: "PRESS", sequence_order: 1 })],
        }),
      ),
    );
  });

  it("starts a new version from the current checkpoints", async () => {
    const template: InspectionTemplate = {
      id: 3,
      entity_sync_id: "t3",
      code: "RND-1",
      name: "Pump house",
      org_scope_id: null,
      route_scope: null,
      estimated_duration_minutes: 20,
      is_active: true,
      current_version_id: 9,
      row_version: 2,
    };
    svc.listInspectionTemplateVersions.mockResolvedValue([
      {
        id: 9,
        entity_sync_id: "v9",
        template_id: 3,
        version_no: 1,
        effective_from: null,
        checkpoint_package_json: "[]",
        tolerance_rules_json: null,
        escalation_rules_json: null,
        requires_review: true,
        row_version: 1,
      },
    ]);
    svc.listInspectionCheckpoints.mockResolvedValue([
      {
        id: 1,
        entity_sync_id: "c1",
        template_version_id: 9,
        sequence_order: 1,
        asset_id: null,
        component_id: null,
        checkpoint_code: "PRESS",
        description: "Pressure",
        check_type: "numeric",
        measurement_unit_id: 21,
        measurement_unit: "bar",
        normal_min: null,
        normal_max: 7.5,
        warning_min: null,
        warning_max: 9,
        requires_photo: false,
        requires_comment_on_exception: false,
        row_version: 1,
      },
    ]);
    render(<TemplateBuilderDialog target={template} onClose={vi.fn()} onSaved={vi.fn()} />);
    expect(await screen.findByDisplayValue("PRESS")).toBeInTheDocument();
    // A warning range narrower than the normal range is refused on the row.
    fireEvent.change(screen.getByLabelText("Warning max"), { target: { value: "7" } });
    fireEvent.click(screen.getByRole("button", { name: "Publish the version" }));
    expect(await screen.findByTestId("row-issues")).toHaveTextContent(
      "The warning range must contain the normal range.",
    );
    expect(svc.publishInspectionTemplateVersion).not.toHaveBeenCalled();
  });
});

describe("InspectionScheduleDialog", () => {
  it("checks the fields before saving and sends a complete schedule", async () => {
    svc.listInspectionTemplates.mockResolvedValue([
      {
        id: 3,
        entity_sync_id: "t3",
        code: "RND-1",
        name: "Pump house",
        org_scope_id: null,
        route_scope: null,
        estimated_duration_minutes: null,
        is_active: true,
        current_version_id: 9,
        row_version: 1,
      },
    ]);
    render(<InspectionScheduleDialog target="new" onClose={vi.fn()} onSaved={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Save the schedule" }));
    expect(
      await screen.findByText("Assign the rounds to a technician or a team."),
    ).toBeInTheDocument();
    expect(screen.getByText("Choose the date and time of the first round.")).toBeInTheDocument();
    expect(svc.saveInspectionSchedule).not.toHaveBeenCalled();

    // Meter trigger: the meter and its interval are asked for.
    fireEvent.click(screen.getByRole("radio", { name: "Meter" }));
    expect(screen.getByText("Choose the meter.")).toBeInTheDocument();
    expect(screen.getByText("Enter an interval greater than zero.")).toBeInTheDocument();
  });

  it("says the asset's meters could not be read instead of 'no meter', and retries", async () => {
    lifecycle.listAssetMeters
      .mockRejectedValueOnce(new Error("db locked"))
      .mockResolvedValue([
        { id: 4, asset_id: 10, name: "Run hours", unit: "h", is_active: true } as never,
      ]);
    render(<InspectionScheduleDialog target="new" onClose={vi.fn()} onSaved={vi.fn()} />);
    fireEvent.click(await screen.findByRole("radio", { name: "Meter" }));
    fireEvent.click(screen.getByTestId("asset-picker"));

    const title = await screen.findByText(enInspections.schedules.metersLoadError);
    expect(screen.queryByText(enInspections.schedules.noMeter)).not.toBeInTheDocument();
    const box = title.closest("[role=alert]") as HTMLElement;
    fireEvent.click(within(box).getByRole("button"));

    await waitFor(() => expect(lifecycle.listAssetMeters).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByText(enInspections.schedules.metersLoadError)).not.toBeInTheDocument(),
    );
    expect(screen.queryByText(enInspections.schedules.noMeter)).not.toBeInTheDocument();
  });
});

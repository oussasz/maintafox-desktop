import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { StudyFmecaStep } from "@/components/reliability/study/StudyFmecaStep";
import { StudyRcmStep } from "@/components/reliability/study/StudyRcmStep";
import { StudySystemStep } from "@/components/reliability/study/StudySystemStep";
import enCommon from "@/i18n/en/common.json";
import enReliability from "@/i18n/locale-data/en/reliability.json";
import { RamsDataQualityPage } from "@/pages/reliability/RamsDataQualityPage";
import type {
  FmecaWorksheet,
  RatingScales,
  RcmRow,
  RcmWorksheet,
  SystemModel,
} from "@/services/reliability-model-service";
import type { ReliabilityStudy } from "@/services/reliability-study-service";
import { useLocaleStore } from "@/stores/locale-store";
import { mockInvoke } from "@/test/mocks/tauri";

// Keys resolve against the real English catalogue: a missing key fails the test.
function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}
const interpolate = (v: string, o?: Record<string, unknown>) =>
  v.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(o?.[n] ?? ""));

function translate(key: string, opts?: Record<string, unknown>): string {
  const count = opts?.["count"];
  const plural = typeof count === "number" ? `${key}_${count === 1 ? "one" : "other"}` : null;
  const value = (plural ? lookup(enReliability, plural) : undefined) ?? lookup(enReliability, key);
  if (value != null) return interpolate(value, opts);
  if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
  throw new Error(`missing reliability i18n key: ${key}`);
}
function translateCommon(key: string, opts?: Record<string, unknown>): string {
  const value = lookup(enCommon, key);
  return value != null ? interpolate(value, opts) : key;
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      ns === "reliability" ? translate(key, opts) : translateCommon(key, opts),
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));
// The Ishikawa canvas has its own tests; here only its placement in step 3 matters.
vi.mock("@/components/reliability/IshikawaDiagramCanvas", () => ({
  IshikawaDiagramCanvas: ({
    equipmentId,
    onPromoteToFmeca,
  }: {
    equipmentId: number;
    onPromoteToFmeca?: (p: { cause: string; diagramId: number; nodeId: string }) => void;
  }) => (
    <div data-testid="ishikawa">
      {equipmentId}
      {onPromoteToFmeca ? (
        <button
          type="button"
          onClick={() => onPromoteToFmeca({ cause: "Loose bolt", diagramId: 9, nodeId: "cause_x" })}
        >
          promote cause
        </button>
      ) : null}
    </div>
  ),
}));

const TS = "2026-10-01T10:00:00Z";

const study: ReliabilityStudy = {
  id: 7,
  title: "P-101, P-102 · Bearing seizure",
  period_start: "2024-10-01T00:00:00Z",
  period_end: TS,
  failure_mode_id: 5,
  failure_mode_code: "BRD",
  failure_mode_label: "Bearing seizure",
  time_basis: "auto",
  planned_cost: null,
  failure_cost: null,
  cost_currency_code: null,
  pm_plan_id: null,
  competing_modes: "ignore",
  mission_hours: null,
  status: "active",
  assets: [
    {
      equipment_id: 10,
      asset_code: "P-101",
      asset_name: "Transfer pump 1",
      class_id: 3,
      class_name: "Centrifugal pump",
    },
    {
      equipment_id: 11,
      asset_code: "P-102",
      asset_name: "Transfer pump 2",
      class_id: 3,
      class_name: "Centrifugal pump",
    },
  ],
  decision_count: 0,
  created_by_name: "A. Analyst",
  row_version: 1,
  created_at: TS,
  updated_at: TS,
};

const grid = (prefix: string) =>
  Array.from({ length: 10 }, (_, i) => ({
    score: i + 1,
    label_fr: `${prefix} ${i + 1}`,
    label_en: `${prefix} ${i + 1}`,
  }));

const scales: RatingScales = {
  version: 1,
  severity: grid("Severity level"),
  occurrence: grid("Occurrence level"),
  detection: grid("Detection level"),
  matrix: {
    classes: 5,
    zones: [
      { min_product: 15, zone: "unacceptable" },
      { min_product: 8, zone: "reduce" },
      { min_product: 4, zone: "monitor" },
      { min_product: 0, zone: "acceptable" },
    ],
  },
};

const weibullProposal = {
  source: "weibull",
  events_per_year: 2.41,
  score: 5,
  failures: 6,
  equipment_years: 4,
  mean_life_hours: 3240,
  hours_per_equipment_year: 7800,
};

const fmeca: FmecaWorksheet = {
  study_id: 7,
  analysis_id: 3,
  rows: [
    {
      item_id: 31,
      row_version: 1,
      component_label: "Drive-end bearing",
      failure_mode_id: 5,
      mode_code: "BRD",
      mode_label: "Bearing seizure",
      functional_failure: "",
      failure_effect: "Pump stops",
      failure_mechanism_id: 70,
      mechanism_label: "Wear",
      failure_cause_id: 80,
      cause_label: "Contamination",
      severity: 7,
      occurrence: 5,
      detectability: 4,
      rpn: 140,
      current_control: "",
      recommended_action: "",
      occurrence_source: "weibull",
      occurrence_events_per_year: 2.41,
      occurrence_proposal: weibullProposal,
      severity_class: 4,
      occurrence_class: 3,
      zone: "reduce",
      rcm_decision_id: null,
      rcm_tactic: null,
      decision_id: null,
      decision_status: null,
      review_required: false,
    },
  ],
  proposals: [
    {
      failure_mode_id: 6,
      code: "ELP",
      label: "External leakage",
      failures_in_period: 2,
      failures_on_class: 5,
      occurrence: {
        source: "history",
        events_per_year: 0.5,
        score: 4,
        failures: 2,
        equipment_years: 4,
        mean_life_hours: null,
        hours_per_equipment_year: null,
      },
      mechanism: { id: 71, code: "2.5", label: "Erosion" },
      cause: null,
    },
  ],
  scales,
  matrix: [{ severity_class: 4, occurrence_class: 3, count: 1, zone: "reduce" }],
  rpn_threshold: 125,
  equipment_years: 4,
  class_names: ["Centrifugal pump"],
  modes: [
    { id: 5, code: "BRD", label: "Bearing seizure" },
    { id: 6, code: "ELP", label: "External leakage" },
  ],
  mechanisms: [
    { id: 70, code: "1.1", label: "Wear" },
    { id: 71, code: "2.5", label: "Erosion" },
  ],
  causes: [{ id: 80, code: "C1", label: "Contamination" }],
  editable: true,
  rows_to_review: 0,
};

beforeEach(() => {
  useLocaleStore.setState({ activeLocale: "en", activeCurrencyCode: "DZD" });
  mockInvoke.mockReset();
});

describe("StudyFmecaStep", () => {
  it("shows the worksheet with the occurrence basis and adds an observed mode", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      if (cmd === "save_reliability_study_fmeca_item") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    const onGoRcm = vi.fn();
    render(<StudyFmecaStep study={study} editable onGoRcm={onGoRcm} />);

    const table = await screen.findByRole("table", { name: /FMECA: Centrifugal pump/ });
    const row = within(table).getByText("Drive-end bearing").closest("tr");
    if (!row) throw new Error("FMECA row missing");
    expect(within(row).getByText("140")).toBeInTheDocument();
    expect(within(row).getByText("Wear · Contamination")).toBeInTheDocument();
    // The proposed occurrence shows where it comes from (life analysis).
    expect(within(row).getByText("Weibull").parentElement).toHaveAttribute(
      "title",
      expect.stringContaining("2.41 failure(s) per equipment-year from the life analysis"),
    );
    expect(screen.getByTestId("ishikawa")).toHaveTextContent("10");
    expect(screen.getByText("Plant critical RPN threshold: 125.")).toBeInTheDocument();

    expect(screen.getByText(/2 in the period · 5 on the class/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText("0.50 failure(s) per equipment-year: 2 over 4.0 equipment-year(s)"),
    ).toBeInTheDocument();
    const save = within(dialog).getByRole("button", { name: "Save the row" });
    expect(save).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Severity"), { target: { value: "6" } });
    fireEvent.change(within(dialog).getByLabelText("Detection"), { target: { value: "3" } });
    expect(within(dialog).getByText("RPN = 72")).toBeInTheDocument();
    fireEvent.click(save);

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_reliability_study_fmeca_item", {
        input: expect.objectContaining({
          study_id: 7,
          item_id: null,
          failure_mode_id: 6,
          failure_mechanism_id: 71,
          failure_cause_id: null,
          severity: 6,
          occurrence: 4,
          detectability: 3,
          occurrence_source: "history",
          occurrence_events_per_year: 0.5,
        }),
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: /RCM decision/ }));
    expect(onGoRcm).toHaveBeenCalled();
  });

  it("flags rows whose ratings nobody chose and says they are not counted", async () => {
    const rated = fmeca.rows[0];
    if (!rated) throw new Error("fixture row missing");
    const placeholder = {
      ...rated,
      item_id: 32,
      component_label: "Coupling",
      severity: 5,
      occurrence: 5,
      detectability: 5,
      rpn: 125,
      review_required: true,
    };
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca")
        return { ...fmeca, rows: [...fmeca.rows, placeholder], rows_to_review: 1 };
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudyFmecaStep study={study} editable onGoRcm={() => undefined} />);
    const table = await screen.findByRole("table", { name: /FMECA: Centrifugal pump/ });
    const row = within(table).getByText("Coupling").closest("tr");
    if (!row) throw new Error("placeholder row missing");
    expect(within(row).getByText("To review")).toBeInTheDocument();
    expect(within(row).queryByText("125")).not.toBeInTheDocument();
    expect(screen.getAllByText("1 row to review not counted").length).toBeGreaterThan(0);
  });

  it("turns a diagram cause into a row the analyst rates, linked to the diagram", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      if (cmd === "save_reliability_study_fmeca_item") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudyFmecaStep study={study} editable onGoRcm={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "promote cause" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/Cause: Loose bolt/)).toBeInTheDocument();
    // Nothing is preset: no mode and no rating, so the row cannot be saved yet.
    const save = within(dialog).getByRole("button", { name: "Save the row" });
    expect(save).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Failure mode"), { target: { value: "5" } });
    fireEvent.change(within(dialog).getByLabelText("Severity"), { target: { value: "6" } });
    fireEvent.change(within(dialog).getByLabelText("Occurrence"), { target: { value: "3" } });
    fireEvent.change(within(dialog).getByLabelText("Detection"), { target: { value: "4" } });
    fireEvent.click(save);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_reliability_study_fmeca_item", {
        input: expect.objectContaining({
          item_id: null,
          failure_mode_id: 5,
          severity: 6,
          occurrence: 3,
          detectability: 4,
          source_ram_ishikawa_diagram_id: 9,
          source_ishikawa_flow_node_id: "cause_x",
        }),
      }),
    );
  });

  it("marks an analyst override when the proposed occurrence is changed", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      if (cmd === "save_reliability_study_fmeca_item") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudyFmecaStep study={study} editable onGoRcm={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit row Bearing seizure" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Occurrence"), { target: { value: "3" } });
    expect(within(dialog).getByRole("button", { name: "Use the proposal" })).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save the row" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_reliability_study_fmeca_item", {
        input: expect.objectContaining({
          item_id: 31,
          expected_row_version: 1,
          occurrence: 3,
          occurrence_source: "analyst",
          occurrence_events_per_year: null,
        }),
      }),
    );
  });
});

const answers = {
  failure_evident: true,
  safety_or_environmental: false,
  operational_impact: true,
  multiple_failure_affects_safety: false,
  on_condition_feasible: false,
  scheduled_restoration_feasible: true,
  scheduled_discard_feasible: true,
  failure_finding_feasible: false,
  combination_feasible: false,
  weibull_beta: 2.1,
};

const timeBased = {
  consequence_category: "operational",
  recommended_tactic: "time_based",
  redesign_compulsory: false,
  path: ["evident:yes", "safety_environmental:no", "operational:yes", "on_condition:no"],
};

const rcmRow: RcmRow = {
  item_id: 31,
  component_label: "Drive-end bearing",
  mode_code: "BRD",
  mode_label: "Bearing seizure",
  functional_failure: "",
  severity: 7,
  occurrence: 5,
  detectability: 4,
  rpn: 140,
  answers,
  prefilled: answers,
  bases: [
    { question: "failure_evident", basis: "detection", score: 4, value: null },
    { question: "operational_impact", basis: "severity", score: 7, value: null },
    {
      question: "scheduled_restoration_feasible",
      basis: "weibull_wear_out",
      score: null,
      value: 2.1,
    },
    { question: "on_condition_feasible", basis: "pf_unknown", score: null, value: null },
  ],
  recommendation: timeBased,
  saved: null,
  pf_interval_hours: null,
  inspection_interval_hours: null,
  replacement_interval_hours: 2160,
  beta: 2.1,
  pattern: "wear_out",
  decision_id: null,
  decision_status: null,
};

const rcm: RcmWorksheet = {
  study_id: 7,
  rcm_study_id: 4,
  rows: [rcmRow],
  editable: true,
  time_basis: "operating",
  rows_to_review: 0,
};

describe("StudyRcmStep", () => {
  it("prefills the SAE JA1011 answers from the FMECA row and the fit, then records the tactic", async () => {
    const saved: RcmWorksheet = {
      ...rcm,
      rows: [
        {
          ...rcmRow,
          answers: { ...answers, on_condition_feasible: true },
          pf_interval_hours: 720,
          saved: {
            decision_id: 90,
            row_version: 1,
            selected_tactic: "condition_based",
            recommended_tactic: "condition_based",
            justification: "",
            path: [],
          },
        },
      ],
    };
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "get_reliability_study_rcm") return rcm;
      if (cmd === "preview_rcm_recommendation") {
        const a = (args as { answers: typeof answers }).answers;
        return a.on_condition_feasible
          ? {
              ...timeBased,
              recommended_tactic: "condition_based",
              path: [...timeBased.path.slice(0, 3), "on_condition:yes"],
            }
          : timeBased;
      }
      if (cmd === "save_reliability_study_rcm_decision") return saved;
      throw new Error(`unexpected ${cmd}`);
    });
    const onDecide = vi.fn();
    render(<StudyRcmStep study={study} editable onDecide={onDecide} onGoFmeca={() => undefined} />);

    expect(
      await screen.findByText("Age-based replacement or restoration", { selector: "p" }),
    ).toBeInTheDocument();
    // Each prefilled answer shows its basis (FMECA rating or Weibull fit).
    expect(
      screen.getByText("From the life analysis: clear wear-out (β = 2.10)."),
    ).toBeInTheDocument();
    expect(screen.getByText("From detection: rating 4 of the grid.")).toBeInTheDocument();
    const toDecisions = screen.getByRole("button", { name: /Send to decisions/ });
    expect(toDecisions).toBeDisabled();

    // A known P-F interval makes condition monitoring feasible.
    fireEvent.change(screen.getByLabelText("P-F interval (operating h)"), {
      target: { value: "720" },
    });
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("preview_rcm_recommendation", {
        answers: expect.objectContaining({ on_condition_feasible: true }),
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: /Tactic retained/ })).toHaveValue(
        "condition_based",
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: /Save the RCM decision/ }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_reliability_study_rcm_decision", {
        input: expect.objectContaining({
          study_id: 7,
          fmeca_item_id: 31,
          selected_tactic: "condition_based",
          pf_interval_hours: 720,
          answers: expect.objectContaining({ on_condition_feasible: true, weibull_beta: 2.1 }),
        }),
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Send to decisions/ })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /Send to decisions/ }));
    // Condition monitoring: inspect at half the P-F interval.
    expect(onDecide).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "one_off_wo", fmecaItemId: 31, rcmDecisionId: 90 }),
    );
    expect(onDecide.mock.calls[0]?.[0].title).toContain("360");
  });

  it("sends the user back to the FMECA when no row exists", async () => {
    mockInvoke.mockResolvedValue({ ...rcm, rows: [] });
    const onGoFmeca = vi.fn();
    render(
      <StudyRcmStep study={study} editable onDecide={() => undefined} onGoFmeca={onGoFmeca} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Go to the FMECA" }));
    expect(onGoFmeca).toHaveBeenCalled();
  });
});

const systemModel: SystemModel = {
  study_id: 7,
  mission_hours: 8760,
  time_basis: "operating",
  fit: [2.1, 3600],
  study_mttr_hours: 7.4,
  equipment: [
    {
      equipment_id: 10,
      asset_code: "P-101",
      asset_name: "Transfer pump 1",
      failures: 6,
      hours: 15600,
      mtbf_hours: 2600,
      mttr_hours: 7.4,
    },
  ],
  rbd: {
    model_id: 12,
    row_version: 1,
    title: "P-101, P-102 · Bearing seizure",
    spec: {
      root_id: "s1",
      nodes: [
        {
          id: "s1",
          label: "Transfer station",
          node_type: "parallel",
          children: ["b1", "b2"],
          k: 1,
          source: null,
          group: null,
          mttr_hours: null,
        },
        {
          id: "b1",
          label: "P-101",
          node_type: "block",
          children: [],
          k: null,
          source: { kind: "study_fit" },
          group: "Centrifugal pump",
          mttr_hours: null,
        },
        {
          id: "b2",
          label: "P-102",
          node_type: "block",
          children: [],
          k: null,
          source: { kind: "equipment_history", equipment_id: 10 },
          group: "Centrifugal pump",
          mttr_hours: null,
        },
      ],
    },
  },
  fta: null,
  editable: true,
};

describe("StudySystemStep", () => {
  it("evaluates the saved RBD with the study fit and shows readable results", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_system_model") return systemModel;
      if (cmd === "evaluate_reliability_study_rbd")
        return {
          mission_hours: 8760,
          system_reliability: 0.412,
          nodes: [
            {
              id: "s1",
              label: "Transfer station",
              node_type: "parallel",
              reliability: 0.412,
              birnbaum: null,
              criticality: null,
              basis: null,
            },
            {
              id: "b1",
              label: "P-101",
              node_type: "block",
              reliability: 0.05,
              birnbaum: 0.38,
              criticality: 0.87,
              basis: {
                kind: "study_fit",
                beta: 2.1,
                eta: 3600,
                mtbf_hours: null,
                failures: null,
                hours: null,
                asset_code: null,
              },
            },
          ],
          groups: [{ group: "Centrifugal pump", share: 1 }],
          issues: [],
        };
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudySystemStep study={study} editable />);

    expect(await screen.findByText("Results over 8,760 h")).toBeInTheDocument();
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("evaluate_reliability_study_rbd", {
        input: expect.objectContaining({ study_id: 7, mission_hours: 8760 }),
      }),
    );
    expect(await screen.findByText("System reliability")).toBeInTheDocument();
    expect(screen.getAllByText("0.412").length).toBeGreaterThan(0);
    // Nothing is written while the step is only opened.
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "save_reliability_study_system_model",
      expect.anything(),
    );
    // Advanced tools open on a guided form, without computing anything yet.
    fireEvent.click(screen.getByRole("button", { name: "Advanced tools" }));
    expect(mockInvoke).not.toHaveBeenCalledWith("evaluate_reliability_markov", expect.anything());
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "simulate_reliability_study_system",
      expect.anything(),
    );
  });
});

describe("RamsDataQualityPage", () => {
  it("lists coding gaps and records operating hours for an equipment", async () => {
    const view = {
      period_start: "2024-10-01T00:00:00Z",
      period_end: TS,
      coding_gaps: [
        {
          work_order_id: 41,
          work_order_code: "OT-2025-0412",
          equipment_id: 10,
          asset_code: "P-101",
          closed_at: "2025-08-03T10:00:00Z",
          gap: "mechanism_missing",
        },
      ],
      no_operating_hours: [
        { equipment_id: 11, asset_code: "P-102", asset_name: "Transfer pump 2", failures: 4 },
      ],
      review_issues: [],
      records: 10,
      records_complete: 9,
    };
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_data_quality") return view;
      if (cmd === "upsert_runtime_exposure_log")
        return {
          id: 1,
          entity_sync_id: "x",
          equipment_id: 11,
          exposure_type: "hours",
          value: 120,
          recorded_at: "2026-09-30T23:59:59Z",
          source_type: "manual",
          row_version: 1,
        };
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <MemoryRouter>
        <RamsDataQualityPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("OT-2025-0412")).toBeInTheDocument();
    expect(screen.getByText("Mechanism missing")).toBeInTheDocument();
    expect(screen.getByText("9 of 10 with mode, cause and mechanism")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Enter operating hours" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Reading date"), {
      target: { value: "2026-09-30" },
    });
    fireEvent.change(within(dialog).getByLabelText("Operating hours"), {
      target: { value: "120" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save the reading" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("upsert_runtime_exposure_log", {
        input: expect.objectContaining({
          equipment_id: 11,
          exposure_type: "hours",
          value: 120,
          // End of the local day, as an instant.
          recorded_at: new Date("2026-09-30T23:59:59").toISOString(),
        }),
      }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });
});

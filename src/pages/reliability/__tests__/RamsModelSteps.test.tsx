import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DecisionDialog } from "@/components/reliability/study/DecisionDialog";
import { RatingScalesDialog } from "@/components/reliability/study/RatingScalesDialog";
import { StudyDecisionsStep } from "@/components/reliability/study/StudyDecisionsStep";
import { StudyFmecaStep } from "@/components/reliability/study/StudyFmecaStep";
import { StudyRcmStep } from "@/components/reliability/study/StudyRcmStep";
import { StudySystemStep } from "@/components/reliability/study/StudySystemStep";
import {
  buildStudyReport,
  studyReportCsvRows,
  studyReportHtml,
} from "@/components/reliability/study/study-report";
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
import type { ReliabilityStudy, StudyDecision } from "@/services/reliability-study-service";
import { useLocaleStore } from "@/stores/locale-store";
import { mockInvoke } from "@/test/mocks/tauri";
import { P } from "@shared/rbac/permissions.generated";

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
// Signed in as user 3.
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 3 } }) }));
let deniedRights: string[] = [];
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => !deniedRights.includes(p),
    canAny: () => true,
    isLoading: false,
  }),
}));
// The Ishikawa canvas has its own tests; here only its placement in step 3 matters.
vi.mock("@/components/reliability/IshikawaDiagramCanvas", () => ({
  IshikawaDiagramCanvas: ({
    equipmentId,
    onPromoteToFmeca,
    onDirtyChange,
  }: {
    equipmentId: number;
    onPromoteToFmeca?: (p: { cause: string; diagramId: number; nodeId: string }) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }) => (
    <div data-testid="ishikawa">
      {equipmentId}
      <button type="button" onClick={() => onDirtyChange?.(true)}>
        move a cause
      </button>
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
      functional_failure: "Low flow",
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
  it("asks before another equipment's diagram replaces unsaved diagram changes", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
    await screen.findByRole("table", { name: /FMECA: Centrifugal pump/ });
    const picker = screen.getByRole("combobox", { name: /Equipment/ });

    // Nothing unsaved: the switch is immediate.
    fireEvent.change(picker, { target: { value: "11" } });
    expect(screen.getByTestId("ishikawa")).toHaveTextContent("11");

    fireEvent.click(screen.getByRole("button", { name: "move a cause" }));
    fireEvent.change(picker, { target: { value: "10" } });
    const ask = await screen.findByRole("dialog", { name: "Discard the diagram changes?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByTestId("ishikawa")).toHaveTextContent("11");

    fireEvent.change(picker, { target: { value: "10" } });
    const again = await screen.findByRole("dialog", { name: "Discard the diagram changes?" });
    fireEvent.click(within(again).getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(screen.getByTestId("ishikawa")).toHaveTextContent("10"));
  });

  it("shows the worksheet with the occurrence basis and adds an observed mode", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      if (cmd === "save_reliability_study_fmeca_item") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    const onGoRcm = vi.fn();
    render(<StudyFmecaStep study={study} onGoRcm={onGoRcm} />);

    const table = await screen.findByRole("table", { name: /FMECA: Centrifugal pump/ });
    const row = within(table).getByText("Drive-end bearing").closest("tr");
    if (!row) throw new Error("FMECA row missing");
    expect(within(row).getByText("140")).toBeInTheDocument();
    expect(within(row).getByText("Wear · Contamination")).toBeInTheDocument();
    expect(within(row).getByText("Function lost: Low flow")).toBeInTheDocument();
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
    fireEvent.change(within(dialog).getByLabelText("Functional failure"), {
      target: { value: "Delivers less than 40 m³/h" },
    });
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
          functional_failure: "Delivers less than 40 m³/h",
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
    render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
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
    render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
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

  it("keeps 'Add another mode' visible but disabled, with the right it needs, and says it is read-only (L2-24)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    deniedRights = [P.RAM_ANALYZE];
    try {
      render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
      await screen.findByRole("table", { name: /FMECA: Centrifugal pump/ });
      const add = screen.getByRole("button", { name: "Add another mode" });
      expect(add).toBeDisabled();
      expect(add).toHaveAttribute("title", expect.stringMatching(/^Right required: /));
      expect(screen.getAllByRole("note")[0]).toHaveTextContent(/^Read-only: /);
    } finally {
      deniedRights = [];
    }
  });

  it("marks an analyst override when the proposed occurrence is changed", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca") return fmeca;
      if (cmd === "save_reliability_study_fmeca_item") return fmeca;
      throw new Error(`unexpected ${cmd}`);
    });
    render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
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
  rpn_threshold: 150,
};

describe("substitute FMECA grids", () => {
  it("warns in the FMECA and RCM steps while the plant grids are to review", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_fmeca")
        return { ...fmeca, scales: { ...fmeca.scales, review: "former_grids_unreadable" } };
      if (cmd === "get_reliability_study_rcm")
        return { ...rcm, grids_review: "former_grids_unreadable" };
      throw new Error(`unexpected ${cmd}`);
    });
    const { unmount } = render(<StudyFmecaStep study={study} onGoRcm={() => undefined} />);
    expect(
      await screen.findByText("Maintafox grids in place, plant grids to review"),
    ).toBeInTheDocument();
    unmount();
    render(
      <StudyRcmStep
        study={study}
        editable
        onDecide={() => undefined}
        onGoFmeca={() => undefined}
      />,
    );
    expect(
      await screen.findByText("Maintafox grids in place, plant grids to review"),
    ).toBeInTheDocument();
  });

  it("says nothing when the plant grids are in place", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_rcm") return rcm;
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyRcmStep
        study={study}
        editable
        onDecide={() => undefined}
        onGoFmeca={() => undefined}
      />,
    );
    expect((await screen.findAllByText(/Bearing seizure/)).length).toBeGreaterThan(0);
    expect(
      screen.queryByText("Maintafox grids in place, plant grids to review"),
    ).not.toBeInTheDocument();
  });
});

describe("StudyRcmStep", () => {
  /** Answers a question of the RCM step by its plain text (en catalogue). */
  const answerQ = (q: keyof typeof enReliability.rams.rcm.q, yes: boolean) =>
    fireEvent.click(
      within(screen.getByRole("group", { name: enReliability.rams.rcm.q[q].text })).getByRole(
        "button",
        { name: yes ? "Yes" : "No" },
      ),
    );

  it("asks every question with no answer ticked in advance, shows the suggestion, then records the tactic (L2-15)", async () => {
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

    // Plain labels, no answer pressed, no recommendation until every question is answered.
    expect(await screen.findByText("Visible failure?")).toBeInTheDocument();
    expect(screen.queryByText("H")).not.toBeInTheDocument();
    for (const b of screen.getAllByRole("button", { name: /^(Yes|No)$/ }))
      expect(b).toHaveAttribute("aria-pressed", "false");
    expect(
      screen.getByText("Answer the 6 remaining questions to get the recommended tactic."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Save the RCM decision/ })).toBeDisabled();
    // The data only suggest an answer, with their basis.
    expect(
      screen.getByText(/From the life analysis: clear wear-out \(β = 2\.10\)\. Suggested: Yes\./),
    ).toBeInTheDocument();
    expect(
      screen.getByText("From detection: rating 4 of the grid. Suggested: Yes."),
    ).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalledWith("preview_rcm_recommendation", expect.anything());

    // A known P-F interval answers "monitoring possible".
    fireEvent.change(screen.getByLabelText("P-F interval (operating h)"), {
      target: { value: "720" },
    });
    answerQ("failure_evident", true);
    answerQ("safety_or_environmental", false);
    answerQ("operational_impact", true);
    answerQ("scheduled_restoration_feasible", true);
    answerQ("scheduled_discard_feasible", true);
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
    // Monitoring at half the P-F interval: the interval is filled, never left as "…".
    expect(screen.getByText(/every 360 operating h/)).toBeInTheDocument();
    expect(screen.queryByText(/…/, { ignore: "script, style, option" })).not.toBeInTheDocument();
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
    expect(onDecide).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "one_off_wo", fmecaItemId: 31, rcmDecisionId: 90 }),
    );
    expect(onDecide.mock.calls[0]?.[0].title).toContain("360");
  });

  it("refuses running a critical mode to failure: flagged, not offered, a task must be chosen (L2-15)", async () => {
    const runToFailure = {
      consequence_category: "operational",
      recommended_tactic: "run_to_failure",
      redesign_compulsory: false,
      path: ["evident:yes", "safety_environmental:no", "operational:yes", "on_condition:no"],
    };
    const critical: RcmWorksheet = {
      ...rcm,
      rows: [{ ...rcmRow, rpn: 392, replacement_interval_hours: null }],
    };
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_rcm") return critical;
      if (cmd === "preview_rcm_recommendation") return runToFailure;
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyRcmStep
        study={study}
        editable
        onDecide={() => undefined}
        onGoFmeca={() => undefined}
      />,
    );
    await screen.findByText("Visible failure?");
    answerQ("failure_evident", true);
    answerQ("safety_or_environmental", false);
    answerQ("operational_impact", true);
    answerQ("on_condition_feasible", false);
    answerQ("scheduled_restoration_feasible", false);
    answerQ("scheduled_discard_feasible", false);
    expect(
      await screen.findByText("Critical mode: RPN 392, plant critical threshold 150"),
    ).toBeInTheDocument();
    const tactic = screen.getByRole("combobox", { name: /Tactic retained/ });
    expect(tactic).toHaveValue("");
    expect(
      screen.getByRole("option", { name: "Run to failure (refused: critical mode)" }),
    ).toBeDisabled();
    const save = screen.getByRole("button", { name: /Save the RCM decision/ });
    expect(save).toBeDisabled();
    fireEvent.change(tactic, { target: { value: "redesign" } });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByLabelText(/Justification/), {
      target: { value: "Bearing seizure stops the line" },
    });
    expect(save).toBeEnabled();
  });

  it("states an age-based task without an established interval in words, never with '…'", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_rcm")
        return { ...rcm, rows: [{ ...rcmRow, replacement_interval_hours: null }] };
      if (cmd === "preview_rcm_recommendation") return timeBased;
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyRcmStep
        study={study}
        editable
        onDecide={() => undefined}
        onGoFmeca={() => undefined}
      />,
    );
    await screen.findByText("Visible failure?");
    answerQ("failure_evident", true);
    answerQ("safety_or_environmental", false);
    answerQ("operational_impact", true);
    answerQ("on_condition_feasible", false);
    answerQ("scheduled_restoration_feasible", true);
    answerQ("scheduled_discard_feasible", true);
    expect(
      await screen.findByText(
        /Step 2 gives no economic interval: set the interval in the decision\./,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/…/, { ignore: "script, style, option" })).not.toBeInTheDocument();
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
      records_missing_mode: 0,
      records_missing_cause: 0,
      records_missing_mechanism: 1,
      org_nodes: [{ id: 4, label: "Usine", depth: 0 }],
      equipment_classes: [{ id: 7, label: "Pump", depth: 0 }],
      coding_gaps_truncated: false,
      no_operating_hours_truncated: false,
      review_issues_truncated: false,
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
    // Opened from the RAMS home page with its period, site and class.
    render(
      <MemoryRouter initialEntries={["/reliability/data-quality?months=6&site=4&class=7"]}>
        <RamsDataQualityPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("OT-2025-0412")).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("get_reliability_data_quality", {
      filter: expect.objectContaining({ org_node_id: 4, equipment_class_id: 7 }),
    });
    expect(screen.getByRole("combobox", { name: "Period" })).toHaveValue("6");
    expect(screen.getByRole("combobox", { name: "Site" })).toHaveValue("4");
    expect(screen.getByRole("combobox", { name: "Class" })).toHaveValue("7");
    expect(screen.getByText("Mechanism missing")).toBeInTheDocument();
    expect(screen.getByText("9 of 10 with mode, cause and mechanism")).toBeInTheDocument();
    // Incomplete count from all records, by missing field; the one listed is the only one.
    expect(
      screen.getByText("0 without mode, 0 without cause, 1 without mechanism"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/not listed here/)).not.toBeInTheDocument();
    expect(screen.queryByText("Every failure of the period is coded.")).not.toBeInTheDocument();

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

  it("counts incomplete imported records that are not in the work-order list", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_data_quality")
        return {
          period_start: "2024-10-01T00:00:00Z",
          period_end: TS,
          coding_gaps: [],
          no_operating_hours: [],
          review_issues: [],
          records: 6,
          records_complete: 3,
          records_missing_mode: 2,
          records_missing_cause: 3,
          records_missing_mechanism: 1,
          org_nodes: [],
          equipment_classes: [],
          coding_gaps_truncated: false,
          no_operating_hours_truncated: false,
          review_issues_truncated: false,
        };
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <MemoryRouter initialEntries={["/reliability/data-quality"]}>
        <RamsDataQualityPage />
      </MemoryRouter>,
    );
    expect(
      await screen.findByText("3 incomplete records not from a work order, not listed here"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("2 without mode, 3 without cause, 1 without mechanism"),
    ).toBeInTheDocument();
    // Never "all coded" while imported records are incomplete.
    expect(screen.queryByText("Every failure of the period is coded.")).not.toBeInTheDocument();
  });
});

const decision = (over: Record<string, unknown>) => ({
  id: 1,
  study_id: 7,
  title: "Decision",
  action_kind: "one_off_wo",
  equipment_id: 10,
  asset_code: "P-101",
  pm_plan_id: null,
  pm_plan_code: null,
  proposed_interval_hours: null,
  proposed_trigger_interval_value: null,
  proposed_trigger_interval_unit: null,
  wo_type_code: "corrective",
  justification: "",
  fmeca_item_id: null,
  rcm_decision_id: null,
  evidence: {
    time_basis: "calendar",
    n_failures: 3,
    n_suspensions: 0,
    beta: null,
    eta: null,
    beta_lower: null,
    beta_upper: null,
    b10: null,
    mean_life: null,
    rho: null,
    pattern: null,
    optimal_interval: null,
    current_interval: null,
    proposed_cost_rate: null,
    current_cost_rate: null,
    run_to_failure_cost_rate: null,
    annual_saving_vs_current: null,
    planned_cost: null,
    failure_cost: null,
    currency_code: null,
    operating_ratio: null,
    study_period_start: TS,
    study_period_end: TS,
    fmeca_mode_label: null,
    rpn: null,
    rcm_tactic: null,
  },
  status: "proposed",
  approver_id: null,
  approver_name: null,
  proposed_by_id: 9,
  proposed_by_name: "B. Planner",
  decided_by_id: null,
  self_approved: false,
  site_allows_self_approval: false,
  proposed_at: TS,
  decided_by_name: null,
  decided_at: null,
  decision_note: null,
  applied_at: null,
  linked_pm_plan_id: null,
  linked_pm_plan_version_id: null,
  linked_pm_version_no: null,
  linked_work_order_id: null,
  linked_work_order_code: null,
  effect: {
    state: "not_applied",
    mtbf_before: null,
    mtbf_after: null,
    after_is_lower_bound: false,
    change: null,
    failures_after: 0,
    hours_after: 0,
  },
  row_version: 1,
  ...over,
});
const decisionFixture = decision({});

describe("StudyDecisionsStep", () => {
  it("never offers the proposer, or someone other than the designated approver, to approve", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({ id: 1, title: "Mine", proposed_by_id: 3 }),
          decision({ id: 2, title: "For J. Lead", approver_id: 8, approver_name: "J. Lead" }),
          decision({ id: 3, title: "Open to me" }),
          decision({
            id: 4,
            title: "Old self-approval",
            status: "approved",
            proposed_by_id: 9,
            decided_by_id: 9,
            self_approved: true,
          }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyDecisionsStep
        study={study}
        version={0}
        onRecord={() => undefined}
        onRevise={() => undefined}
      />,
    );

    const rowOf = async (title: string) => {
      const row = (await screen.findByText(title)).closest("tr");
      if (!row) throw new Error(`row ${title} missing`);
      return within(row);
    };
    const mine = await rowOf("Mine");
    expect(mine.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(mine.getByText("You proposed it: another person approves.")).toBeInTheDocument();
    // The proposer's own rejection is a withdrawal (owner decision D10).
    expect(mine.getByRole("button", { name: "Withdraw" })).toBeInTheDocument();
    expect(mine.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();

    const forLead = await rowOf("For J. Lead");
    expect(forLead.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(forLead.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
    expect(forLead.getByText("Awaiting J. Lead")).toBeInTheDocument();

    const open = await rowOf("Open to me");
    expect(open.getByRole("button", { name: "Approve" })).toBeInTheDocument();
    expect(open.queryByText("Self-approved")).not.toBeInTheDocument();

    const old = await rowOf("Old self-approval");
    expect(old.getByTitle("Approved by the person who proposed it.")).toHaveTextContent(
      "Self-approved",
    );
  });

  it("lets the proposer withdraw their proposal whoever was designated, and shows it withdrawn", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({
            id: 1,
            title: "Mine, for J. Lead",
            proposed_by_id: 3,
            approver_id: 8,
            approver_name: "J. Lead",
          }),
          decision({
            id: 2,
            title: "Withdrawn one",
            status: "rejected",
            proposed_by_id: 9,
            decided_by_id: 9,
          }),
          decision({
            id: 3,
            title: "Rejected one",
            status: "rejected",
            proposed_by_id: 9,
            decided_by_id: 8,
          }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyDecisionsStep
        study={study}
        version={0}
        onRecord={() => undefined}
        onRevise={() => undefined}
      />,
    );
    const rowOf = async (title: string) => {
      const row = (await screen.findByText(title)).closest("tr");
      if (!row) throw new Error(`row ${title} missing`);
      return within(row);
    };
    const mine = await rowOf("Mine, for J. Lead");
    expect(mine.getByText("Awaiting J. Lead")).toBeInTheDocument();
    expect(mine.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(mine.getByRole("button", { name: "Withdraw" })).toBeInTheDocument();
    expect((await rowOf("Withdrawn one")).getByText("Withdrawn")).toBeInTheDocument();
    expect((await rowOf("Rejected one")).getByText("Rejected")).toBeInTheDocument();
  });

  it("lets the proposer approve when the decision's site opted out of four-eyes", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({ id: 1, title: "Mine", proposed_by_id: 3, site_allows_self_approval: true }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyDecisionsStep
        study={study}
        version={0}
        onRecord={() => undefined}
        onRevise={() => undefined}
      />,
    );
    const row = (await screen.findByText("Mine")).closest("tr");
    if (!row) throw new Error("row missing");
    expect(within(row).getByRole("button", { name: "Approve" })).toBeInTheDocument();
    expect(
      within(row).queryByText("You proposed it: another person approves."),
    ).not.toBeInTheDocument();
  });

  it("keeps 'Record a decision' visible but disabled and notes that approving needs ram.manage (L2-24)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [decision({ id: 1, title: "Interval", status: "proposed" })];
      throw new Error(`unexpected ${cmd}`);
    });
    deniedRights = [P.RAM_ANALYZE, P.RAM_MANAGE];
    try {
      render(
        <StudyDecisionsStep
          study={study}
          version={0}
          onRecord={() => undefined}
          onRevise={() => undefined}
        />,
      );
      await screen.findByText("Interval");
      const record = screen.getByRole("button", { name: "Record a decision" });
      expect(record).toBeDisabled();
      expect(record).toHaveAttribute("title", expect.stringMatching(/^Right required: /));
      expect(screen.getByRole("note")).toHaveTextContent(/^Read-only: /);
    } finally {
      deniedRights = [];
    }
  });

  it("offers Apply only with the right of the module it writes in (AU-85)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({ id: 1, title: "Interval", action_kind: "pm_interval", status: "approved" }),
          decision({ id: 2, title: "Repair", status: "approved" }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    deniedRights = [P.PM_EDIT];
    try {
      render(
        <StudyDecisionsStep
          study={study}
          version={0}
          onRecord={() => undefined}
          onRevise={() => undefined}
        />,
      );
      const interval = within((await screen.findByText("Interval")).closest("tr") as HTMLElement);
      expect(interval.queryByRole("button", { name: "Apply" })).not.toBeInTheDocument();
      expect(
        interval.getByText("Applying requires the right to edit preventive plans."),
      ).toBeInTheDocument();
      const repair = within(screen.getByText("Repair").closest("tr") as HTMLElement);
      expect(repair.getByRole("button", { name: "Apply" })).toBeInTheDocument();
    } finally {
      deniedRights = [];
    }
  });

  it("shows a decision refused at synchronisation with its reason and offers neither Approve nor Apply (AU-76b)", async () => {
    const reason = "refusé (sync) : contenu modifié après approbation — vérifié avec le site";
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({
            id: 1,
            title: "Refused approved",
            status: "approved",
            sync_refused_at: "2026-10-26T10:00:00Z",
            sync_refused_reason: reason,
          }),
          decision({
            id: 2,
            title: "Refused proposed",
            sync_refused_at: "2026-10-26T10:00:00Z",
            sync_refused_reason: reason,
          }),
          decision({ id: 3, title: "Fine approved", status: "approved" }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <StudyDecisionsStep
        study={study}
        version={0}
        onRecord={() => undefined}
        onRevise={() => undefined}
      />,
    );
    const approved = within(
      (await screen.findByText("Refused approved")).closest("tr") as HTMLElement,
    );
    expect(approved.getByText("Refused at synchronisation")).toBeInTheDocument();
    expect(approved.getByText(reason)).toBeInTheDocument();
    expect(approved.queryByRole("button", { name: "Apply" })).not.toBeInTheDocument();
    const proposed = within(screen.getByText("Refused proposed").closest("tr") as HTMLElement);
    expect(proposed.getByText("Refused at synchronisation")).toBeInTheDocument();
    expect(proposed.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    const fine = within(screen.getByText("Fine approved").closest("tr") as HTMLElement);
    expect(fine.queryByText("Refused at synchronisation")).not.toBeInTheDocument();
    expect(fine.getByRole("button", { name: "Apply" })).toBeInTheDocument();
  });

  it("shows who approves, who decided, why, the saving in a currency and both interval units; the proposer edits (L2-16)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_reliability_study_decisions")
        return [
          decision({
            id: 1,
            title: "Mine, interval",
            proposed_by_id: 3,
            action_kind: "pm_interval",
            pm_plan_code: "PM-PMP-014",
            proposed_interval_hours: 270,
            proposed_trigger_interval_value: 24,
            proposed_trigger_interval_unit: "day",
            justification: "Two seizures in six months",
            evidence: {
              ...decision({}).evidence,
              time_basis: "operating",
              annual_saving_vs_current: 4400000,
            },
          }),
          decision({
            id: 2,
            title: "Approved one",
            status: "approved",
            approver_id: 8,
            approver_name: "J. Lead",
            decided_by_id: 8,
            decided_by_name: "J. Lead",
            decided_at: TS,
          }),
        ];
      throw new Error(`unexpected ${cmd}`);
    });
    const onRevise = vi.fn();
    render(
      <StudyDecisionsStep
        study={study}
        version={0}
        onRecord={() => undefined}
        onRevise={onRevise}
      />,
    );
    const mine = within((await screen.findByText("Mine, interval")).closest("tr") as HTMLElement);
    expect(mine.getByText(/approver: anyone authorised/)).toBeInTheDocument();
    expect(mine.getByText("Justification: Two seizures in six months")).toBeInTheDocument();
    // "24 days" and "270 h" read as one value.
    expect(
      mine.getByText(/new interval 24 day\(s\) \(that is 270 operating h of the study/),
    ).toBeInTheDocument();
    // The saving carries a currency (the plant's when the costs named none).
    const saving = mine.getByText(/saving .*4,400,000.*\/yr/);
    expect(saving.textContent).not.toMatch(/saving 4,400,000\/yr/);
    fireEvent.click(mine.getByRole("button", { name: "Edit the decision Mine, interval" }));
    expect(onRevise).toHaveBeenCalledWith(expect.objectContaining({ id: 1 }));

    const approved = within(screen.getByText("Approved one").closest("tr") as HTMLElement);
    expect(approved.getByText(/^Approved by J\. Lead on /)).toBeInTheDocument();
    expect(approved.queryByRole("button", { name: /^Edit the decision/ })).not.toBeInTheDocument();
  });
});

describe("DecisionDialog", () => {
  const emptyAnalysis = {
    effective_time_basis: "operating",
    n_failures: 0,
    n_suspensions: 0,
    min_failures: 5,
    fit: null,
    fit_issue: "not_enough_failures",
    plot_points: [],
    band: [],
    curve: [],
    cost: null,
    cost_source: "missing",
    cost_currency_code: null,
    cost_class_id: null,
    cost_class_name: null,
    pm_plans: [],
    selected_pm_plan_id: null,
    operating_hours_per_year: null,
    asset_count: 2,
    operating_ratio: null,
  };
  const emergency = {
    id: 1,
    code: "emergency",
    label: "Dépannage d'urgence",
    label_en: "Emergency repair",
    nature: null,
    nature_label: null,
    nature_label_en: null,
    is_system: true,
    is_active: true,
  };
  const mock = () =>
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_study_life_analysis") return emptyAnalysis;
      if (cmd === "list_work_order_types") return [emergency];
      if (cmd === "list_reliability_decision_approvers") return [];
      if (
        cmd === "create_reliability_study_decision" ||
        cmd === "revise_reliability_study_decision"
      )
        return decisionFixture;
      throw new Error(`unexpected ${cmd}`);
    });

  it("presets no work-order type and needs a justification (L2-16)", async () => {
    mock();
    render(
      <DecisionDialog
        open
        onOpenChange={() => undefined}
        study={study}
        prefill={null}
        onCreated={() => undefined}
      />,
    );
    const type = await screen.findByRole("combobox", { name: "Work order type" });
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Emergency repair" })).toBeInTheDocument(),
    );
    expect(type).toHaveValue("");
    expect(screen.getByRole("option", { name: "Choose the work-order type…" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Decision" }), {
      target: { value: "Alignment check" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Equipment" }), {
      target: { value: "10" },
    });
    fireEvent.change(type, { target: { value: "emergency" } });
    const submit = screen.getByRole("button", { name: "Send for approval" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "Justification (required)" }), {
      target: { value: "Misalignment found on both pumps" },
    });
    expect(submit).toBeEnabled();
    expect(
      screen.getByText(/anyone holding the right to approve reliability decisions may approve it/),
    ).toBeInTheDocument();
    fireEvent.click(submit);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("create_reliability_study_decision", {
        input: expect.objectContaining({
          title: "Alignment check",
          wo_type_code: "emergency",
          equipment_id: 10,
          justification: "Misalignment found on both pumps",
        }),
      }),
    );
  });

  it("edits a proposal: the corrected decision replaces it, with what changes (L2-16)", async () => {
    mock();
    const onCreated = vi.fn();
    render(
      <DecisionDialog
        open
        onOpenChange={() => undefined}
        study={study}
        prefill={null}
        revising={{
          ...(decisionFixture as StudyDecision),
          id: 41,
          row_version: 3,
          title: "Alignment check",
          equipment_id: 10,
          wo_type_code: "emergency",
          justification: "Misalignment",
        }}
        onCreated={onCreated}
      />,
    );
    expect(await screen.findByText("Edit the decision")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Decision" })).toHaveValue("Alignment check");
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Work order type" })).toHaveValue("emergency"),
    );
    const submit = screen.getByRole("button", { name: "Save the change" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "What changes (required)" }), {
      target: { value: "Both pumps, not one" },
    });
    fireEvent.click(submit);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("revise_reliability_study_decision", {
        input: expect.objectContaining({
          decision_id: 41,
          expected_row_version: 3,
          reason: "Both pumps, not one",
          decision: expect.objectContaining({
            title: "Alignment check",
            justification: "Misalignment",
          }),
        }),
      }),
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
  });
});

describe("study report (L2-14)", () => {
  const f = {
    t: translate,
    number: (v: number) => v.toLocaleString("en"),
    decimal: (v: number, d: number) => v.toFixed(d),
    money: (v: number) => `DZD ${v.toLocaleString("en")}`,
    date: (v: string) => v.slice(0, 10),
  };
  it("puts the scope, the decision note and the decisions in the print and the CSV", () => {
    const analysis = {
      effective_time_basis: "operating",
      n_failures: 8,
      n_suspensions: 2,
      min_failures: 5,
      fit: {
        beta: 3.04,
        eta: 4440,
        confidence: 0.9,
        beta_lower: 1.9,
        beta_upper: 4.8,
        eta_lower: 3700,
        eta_upper: 5300,
        b10: 2118,
        b10_lower: null,
        b10_upper: null,
        mean_life: 3967,
        median_life: 3940,
        rho: 0.9996,
        pattern: "wear_out",
      },
      fit_issue: null,
      plot_points: [],
      band: [],
      curve: [],
      cost: {
        planned_cost: 180000,
        failure_cost: 1450000,
        run_to_failure: { interval: null, cost_rate: 365.5, annual_cost: 5848000 },
        current: null,
        proposed: { interval: 1852, cost_rate: 146.1, annual_cost: 2337600 },
        rejection: null,
        saving_vs_run_to_failure: 0.6,
        annual_saving_vs_current: null,
        curve: [],
      },
      cost_source: "study",
      cost_currency_code: "DZD",
      cost_class_id: null,
      cost_class_name: null,
      pm_plans: [],
      selected_pm_plan_id: null,
      operating_hours_per_year: 16000,
      asset_count: 2,
      operating_ratio: 0.91,
    };
    const d = {
      ...(decisionFixture as StudyDecision),
      title: "Fixed-age replacement every 1,900 h",
      justification: "Wear-out shown",
    };
    const report = buildStudyReport("study", study, analysis, [d], f);
    expect(report.title).toBe(`Reliability study: ${study.title}`);
    const rows = studyReportCsvRows(report);
    expect(rows).toContainEqual([
      "Scope",
      "Equipment",
      "P-101 · Transfer pump 1, P-102 · Transfer pump 2",
    ]);
    expect(
      rows.some((r) => String(r[2]).includes("Replace the part of P-101, P-102 every 1,900")),
    ).toBe(true);
    expect(
      rows.some((r) => r[1] === d.title && String(r[2]).includes("Justification: Wear-out shown")),
    ).toBe(true);
    const html = studyReportHtml(report, "", "Printed on 2026-11-11", "en");
    expect(html).toContain("<h2>Decision note for management</h2>");
    expect(html).toContain("DZD 180,000");
    // The note alone leaves the life details and the decision log out.
    const note = buildStudyReport("note", study, analysis, [], f);
    expect(note.sections.map((s) => s.title)).toEqual(["Scope", "Decision note for management"]);
  });
});

describe("RatingScalesDialog", () => {
  it("says the grids are plant-wide and keeps the last valid threshold when the text is not a number", () => {
    render(
      <RatingScalesDialog
        open
        onOpenChange={() => undefined}
        scales={scales}
        editable
        onSaved={() => undefined}
      />,
    );
    expect(screen.getByRole("note")).toHaveTextContent(/shared by every FMECA study/);
    fireEvent.click(screen.getByRole("tab", { name: "Occurrence" }));
    const threshold = screen.getByRole("textbox", {
      name: "Threshold of rating 3 (failures per year)",
    });
    fireEvent.change(threshold, { target: { value: "0,5x" } });
    // The field flags the text, keeps the last valid threshold, and gives it back on blur.
    expect(threshold).toHaveAttribute("aria-invalid", "true");
    fireEvent.blur(threshold);
    expect(threshold).not.toHaveAttribute("aria-invalid");
    expect(screen.getByRole("button", { name: "Save the grids" })).toBeEnabled();

    // A decimal with a comma is accepted.
    fireEvent.change(threshold, { target: { value: "0,5" } });
    expect(threshold).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save the grids" })).toBeEnabled();
  });
});

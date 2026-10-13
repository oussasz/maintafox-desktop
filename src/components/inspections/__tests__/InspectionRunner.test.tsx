import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enInspections from "@/i18n/locale-data/en/inspections.json";
import type {
  InspectionCheckpoint,
  InspectionRoundRunner,
  InspectionRunnerCheckpoint,
} from "@shared/ipc-types";

import { InspectionRunner } from "../InspectionRunner";

// Resolve keys against the real English catalogue so a missing key fails the test.
function translate(key: string, opts?: Record<string, unknown>): string {
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
  getInspectionRoundRunner: vi.fn(),
  recordInspectionResult: vi.fn(),
  addInspectionPhotoEvidence: vi.fn(),
  completeInspectionRound: vi.fn(),
  reviewInspectionRound: vi.fn(),
  routeInspectionAnomalyToDi: vi.fn(),
  routeInspectionAnomalyToWo: vi.fn(),
  deferInspectionAnomaly: vi.fn(),
}));
vi.mock("@/services/inspection-service", () => svc);
vi.mock("@/services/wo-service", () => ({
  listWorkOrderTypes: vi
    .fn()
    .mockResolvedValue([
      { id: 1, code: "corrective", label: "Corrective", is_system: true, is_active: true },
    ]),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

function checkpoint(id: number, partial: Partial<InspectionCheckpoint>): InspectionCheckpoint {
  return {
    id,
    entity_sync_id: `cp-${id}`,
    template_version_id: 1,
    sequence_order: id,
    asset_id: null,
    component_id: null,
    checkpoint_code: `CP-${id}`,
    description: null,
    check_type: "pass_fail",
    measurement_unit: null,
    normal_min: null,
    normal_max: null,
    warning_min: null,
    warning_max: null,
    requires_photo: false,
    requires_comment_on_exception: false,
    row_version: 1,
    ...partial,
  };
}

function runnerRow(
  cp: InspectionCheckpoint,
  partial: Partial<InspectionRunnerCheckpoint> = {},
): InspectionRunnerCheckpoint {
  return {
    checkpoint: cp,
    asset_code: null,
    asset_name: null,
    result: null,
    evidence: [],
    open_anomalies: [],
    sync_state: "not_recorded",
    missing: ["result"],
    ...partial,
  };
}

function runner(status: string, rows: InspectionRunnerCheckpoint[]): InspectionRoundRunner {
  return {
    summary: {
      round: {
        id: 42,
        entity_sync_id: "r-42",
        template_id: 3,
        template_version_id: 1,
        scheduled_at: null,
        assigned_to_id: null,
        status,
        started_at: null,
        completed_at: null,
        reviewed_at: null,
        reviewed_by_id: null,
        review_note: null,
        completion_percentage: null,
        row_version: 5,
      },
      template_code: "RND-PUMPS",
      template_name: "Pump house round",
      version_no: 1,
      requires_review: false,
      assigned_to_name: "Amina Tech",
      reviewed_by_name: null,
      checkpoint_count: rows.length,
      recorded_count: rows.filter((r) => r.result).length,
      exception_count: 0,
      open_anomaly_count: 0,
      pending_sync_count: 0,
    },
    checkpoints: rows,
  };
}

const temperature = checkpoint(1, {
  checkpoint_code: "TEMP-BRG",
  description: "Bearing temperature, drive end",
  check_type: "numeric",
  measurement_unit: "°C",
  normal_max: 70,
  warning_max: 85,
  requires_comment_on_exception: true,
});
const leak = checkpoint(2, { checkpoint_code: "VIS-LEAK", description: "No visible leak" });

const savedResult = {
  id: 9,
  entity_sync_id: "res-9",
  round_id: 42,
  checkpoint_id: 2,
  result_status: "pass",
  numeric_value: null,
  text_value: null,
  boolean_value: null,
  comment: null,
  recorded_at: "2026-10-12T06:10:00Z",
  recorded_by_id: 1,
  row_version: 1,
};

describe("InspectionRunner", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows labels and the offline queue, never raw ids", async () => {
    svc.getInspectionRoundRunner.mockResolvedValue(
      runner("in_progress", [
        runnerRow(temperature, { asset_code: "PMP-101", asset_name: "Feed pump 101" }),
        runnerRow(leak, { result: savedResult, sync_state: "waiting", missing: [] }),
      ]),
    );
    render(<InspectionRunner roundId={42} onClose={vi.fn()} />);
    expect(await screen.findByText("Pump house round")).toBeInTheDocument();
    expect(screen.getByText("Bearing temperature, drive end")).toBeInTheDocument();
    expect(screen.getByText("PMP-101")).toBeInTheDocument();
    expect(screen.getByTestId("inspection-sync-banner")).toHaveTextContent(
      "1 result saved on this device, waiting to sync.",
    );
    expect(screen.queryByText(/res-9|cp-1|r-42/)).not.toBeInTheDocument();
  });

  it("previews the status of a reading and requires a comment out of tolerance", async () => {
    svc.getInspectionRoundRunner.mockResolvedValue(
      runner("in_progress", [runnerRow(temperature), runnerRow(leak)]),
    );
    svc.recordInspectionResult.mockResolvedValue({ ...savedResult, checkpoint_id: 1 });
    render(<InspectionRunner roundId={42} onClose={vi.fn()} />);
    const input = await screen.findByLabelText("Reading");
    fireEvent.change(input, { target: { value: "80,5" } });
    expect(screen.getByTestId("predicted-status")).toHaveTextContent("Warning");
    const save = screen.getByRole("button", { name: "Save and next" });
    expect(save).toBeDisabled();
    expect(screen.getByText("A comment is required for this answer.")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Comment"), { target: { value: "Warm" } });
    expect(save).toBeEnabled();
    fireEvent.click(save);
    await waitFor(() => expect(svc.recordInspectionResult).toHaveBeenCalledTimes(1));
    expect(svc.recordInspectionResult).toHaveBeenCalledWith(
      expect.objectContaining({
        round_id: 42,
        checkpoint_id: 1,
        numeric_value: 80.5,
        result_status: null,
        comment: "Warm",
      }),
    );
  });

  it("asks for a reason when a check cannot be performed", async () => {
    svc.getInspectionRoundRunner.mockResolvedValue(runner("in_progress", [runnerRow(leak)]));
    svc.recordInspectionResult.mockResolvedValue(savedResult);
    render(<InspectionRunner roundId={42} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("radio", { name: "Not accessible" }));
    expect(screen.getByLabelText("Reason (required)")).toBeInTheDocument();
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason (required)"), {
      target: { value: "Area fenced off" },
    });
    fireEvent.click(save);
    await waitFor(() =>
      expect(svc.recordInspectionResult).toHaveBeenCalledWith(
        expect.objectContaining({ result_status: "not_accessible", numeric_value: null }),
      ),
    );
  });

  it("completes the round only when every check is done", async () => {
    svc.getInspectionRoundRunner.mockResolvedValue(
      runner("in_progress", [
        runnerRow(leak, { result: savedResult, sync_state: "synced", missing: [] }),
      ]),
    );
    svc.completeInspectionRound.mockResolvedValue({});
    render(<InspectionRunner roundId={42} onClose={vi.fn()} />);
    const complete = await screen.findByRole("button", { name: "Complete the round" });
    expect(complete).toBeEnabled();
    fireEvent.click(complete);
    await waitFor(() =>
      expect(svc.completeInspectionRound).toHaveBeenCalledWith({
        round_id: 42,
        expected_row_version: 5,
      }),
    );
  });

  it("is read-only once completed and lists the findings to decide", async () => {
    const anomaly = {
      id: 77,
      round_id: 42,
      result_id: 9,
      anomaly_type: "threshold_breach",
      severity: 2,
      description: "Result outside normal or warning tolerance bands.",
      linked_di_id: null,
      linked_work_order_id: null,
      requires_permit_review: false,
      resolution_status: "open",
      routing_decision: null,
      entity_sync_id: "an-77",
      row_version: 1,
    };
    svc.getInspectionRoundRunner.mockResolvedValue(
      runner("completed_with_findings", [
        runnerRow(leak, {
          result: savedResult,
          sync_state: "synced",
          missing: [],
          open_anomalies: [anomaly],
        }),
      ]),
    );
    svc.deferInspectionAnomaly.mockResolvedValue({ ...anomaly, resolution_status: "deferred" });
    render(<InspectionRunner roundId={42} onClose={vi.fn()} />);
    expect(await screen.findByText(/results can no longer be changed/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "OK" })).toBeDisabled();
    expect(screen.getByTestId("review-finding")).toHaveTextContent("Out of tolerance");
    expect(screen.getByRole("button", { name: "Mark as reviewed" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Defer" }));
    await waitFor(() =>
      expect(svc.deferInspectionAnomaly).toHaveBeenCalledWith({
        anomaly_id: 77,
        expected_row_version: 1,
      }),
    );
  });
});

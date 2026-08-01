/**
 * Supervisor Verification — Phase 2 - SP04 - F01 - Sprint S3
 *
 * V1 — Type assignment: assigning "invalid_status" to DiStatus produces a TS error.
 * V2 — Zod validation fires: a response missing 'code' field causes Zod parse to throw.
 */

import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { DiStatus } from "@shared/ipc-types";

import { listDis, getDi, createDi, updateDiDraft } from "../di-service";

// ── Fixture ───────────────────────────────────────────────────────────────────

function makeDiResponse(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 1,
    code: "DI-0001",
    asset_id: 10,
    sub_asset_ref: null,
    org_node_id: 5,
    status: "submitted",
    title: "Fuite pompe",
    description: "Fuite détectée sur la pompe principale",
    origin_type: "operator",
    symptom_code_id: null,
    impact_level: "minor",
    production_impact: false,
    safety_flag: false,
    environmental_flag: false,
    quality_flag: false,
    reported_urgency: "medium",
    validated_urgency: null,
    observed_at: null,
    submitted_at: "2026-04-01T10:00:00Z",
    review_team_id: null,
    reviewer_id: null,
    screened_at: null,
    approved_at: null,
    deferred_until: null,
    declined_at: null,
    closed_at: null,
    archived_at: null,
    converted_to_wo_id: null,
    converted_at: null,
    reviewer_note: null,
    classification_code_id: null,
    is_recurrence_flag: false,
    recurrence_di_id: null,
    row_version: 1,
    submitter_id: 42,
    created_at: "2026-04-01T10:00:00Z",
    updated_at: "2026-04-01T10:00:00Z",
    ...overrides,
  };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("di-service — Supervisor Verification S3", () => {
  // ── V1 — Type assignment ──────────────────────────────────────────────────

  it("V1 — DiStatus type rejects invalid string literals at compile time", () => {
    // This is a compile-time check.  The @ts-expect-error directive proves
    // that TypeScript would flag the assignment as an error.
    // If DiStatus ever became `string`, the @ts-expect-error itself would
    // fail and vitest/tsc would report "Unused @ts-expect-error directive".
    // @ts-expect-error — "invalid_status" is not assignable to DiStatus
    const _unusedStatus: DiStatus = "invalid_status";
    void _unusedStatus;
    expect(true).toBe(true); // runtime placeholder
  });

  // ── V2 — Zod validation fires ────────────────────────────────────────────

  it("V2 — Zod throws when response is missing required 'code' field", async () => {
    const malformed = makeDiResponse();

    delete (malformed as Record<string, unknown>)["code"]; // remove required field

    mockInvoke.mockResolvedValueOnce(malformed);

    await expect(
      createDi({
        asset_id: 10,
        org_node_id: 5,
        title: "Test",
        description: "Test desc",
        origin_type: "operator",
        impact_level: "minor",
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: "medium",
        submitter_id: 42,
      }),
    ).rejects.toThrow();
  });

  it("V2b — listDis validates items shape via Zod", async () => {
    const malformedPage = {
      items: [makeDiResponse({ code: undefined })],
      total: 1,
    };
    mockInvoke.mockResolvedValueOnce(malformedPage);

    await expect(listDis({ limit: 50, offset: 0 })).rejects.toThrow();
  });

  it("V2c — getDi validates composite response via Zod", async () => {
    const malformedDetail = {
      di: makeDiResponse({ id: undefined }), // missing required field
      transitions: [],
      similar: [],
    };
    mockInvoke.mockResolvedValueOnce(malformedDetail);

    await expect(getDi(1)).rejects.toThrow();
  });

  // ── Smoke: well-formed responses parse correctly ──────────────────────────

  it("listDis parses a valid page response", async () => {
    const page = { items: [makeDiResponse()], total: 1 };
    mockInvoke.mockResolvedValueOnce(page);

    const result = await listDis({ limit: 50, offset: 0 });
    expect(result.total).toBe(1);
    expect(result.items[0]?.code).toBe("DI-0001");
    expect(mockInvoke).toHaveBeenCalledWith("list_di", {
      filter: { limit: 50, offset: 0 },
    });
  });

  it("getDi parses a valid detail response", async () => {
    const detail = {
      di: makeDiResponse(),
      transitions: [
        {
          id: 1,
          from_status: "none",
          to_status: "submitted",
          action: "submit",
          actor_id: 42,
          reason_code: null,
          notes: null,
          acted_at: "2026-04-01T10:00:00Z",
        },
      ],
      similar: [],
    };
    mockInvoke.mockResolvedValueOnce(detail);

    const result = await getDi(1);
    expect(result.di.id).toBe(1);
    expect(result.transitions).toHaveLength(1);
    expect(mockInvoke).toHaveBeenCalledWith("get_di", { id: 1 });
  });

  it("createDi sends correct invoke name", async () => {
    mockInvoke.mockResolvedValueOnce(makeDiResponse());

    const input = {
      asset_id: 10,
      org_node_id: 5,
      title: "Test",
      description: "Desc",
      origin_type: "operator",
      impact_level: "minor",
      production_impact: false,
      safety_flag: false,
      environmental_flag: false,
      quality_flag: false,
      reported_urgency: "medium",
      submitter_id: 42,
    };
    await createDi(input);
    expect(mockInvoke).toHaveBeenCalledWith("create_di", { input });
  });

  it("updateDiDraft sends correct invoke name", async () => {
    mockInvoke.mockResolvedValueOnce(makeDiResponse({ row_version: 2 }));

    const input = {
      id: 1,
      expected_row_version: 1,
      title: "Updated",
    };
    const result = await updateDiDraft(input);
    expect(result.row_version).toBe(2);
    expect(mockInvoke).toHaveBeenCalledWith("update_di_draft", { input });
  });
});

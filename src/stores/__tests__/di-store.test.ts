/**
 * Supervisor Verification — Phase 2 - SP04 - F01 - Sprint S3
 *
 * V3 — Store filter merge: setFilter merges partial updates into existing filter.
 * V4 — Loading state: loadDis sets loading=true while awaiting and loading=false after.
 */

import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { InterventionRequest } from "@shared/ipc-types";

import { useDiStore } from "../di-store";

// ── Fixture ───────────────────────────────────────────────────────────────────

function makeDi(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 1,
    code: "DI-0001",
    asset_id: 10,
    sub_asset_ref: null,
    org_node_id: 5,
    status: "submitted",
    title: "Fuite pompe",
    description: "Fuite détectée",
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

function resetStore() {
  useDiStore.setState({
    items: [],
    total: 0,
    activeDi: null,
    filter: { limit: 50, offset: 0 },
    loading: false,
    saving: false,
    error: null,
  });
}

// ── Tests ─────────────────────────────────────────────────────────────────────

beforeEach(() => {
  mockInvoke.mockReset();
  resetStore();
});

describe("di-store — Supervisor Verification S3", () => {
  // ── V3 — Store filter merge ───────────────────────────────────────────────

  describe("V3 — setFilter merges partial updates", () => {
    it("merges status filter then asset_id filter", () => {
      const { setFilter } = useDiStore.getState();

      setFilter({ status: ["submitted"] });
      setFilter({ asset_id: 5 });

      const { filter } = useDiStore.getState();
      expect(filter.status).toEqual(["submitted"]);
      expect(filter.asset_id).toBe(5);
      // Default fields preserved
      expect(filter.limit).toBe(50);
      expect(filter.offset).toBe(0);
    });

    it("overwrites existing field on second call", () => {
      const { setFilter } = useDiStore.getState();

      setFilter({ status: ["submitted"] });
      setFilter({ status: ["submitted", "screened"] });

      const { filter } = useDiStore.getState();
      expect(filter.status).toEqual(["submitted", "screened"]);
    });
  });

  // ── V4 — Loading state ───────────────────────────────────────────────────

  describe("V4 — loading flag lifecycle", () => {
    it("loading=true while loadDis is awaiting, loading=false after", async () => {
      // Create a deferred promise so we can observe the intermediate state
      let resolveInvoke!: (value: unknown) => void;
      mockInvoke.mockReturnValueOnce(
        new Promise((resolve) => {
          resolveInvoke = resolve;
        }),
      );

      // Start load — do NOT await yet
      const loadPromise = useDiStore.getState().loadDis();

      // Intermediate: loading should be true
      expect(useDiStore.getState().loading).toBe(true);
      expect(useDiStore.getState().error).toBeNull();

      // Resolve the invoke call
      resolveInvoke({ items: [makeDi()], total: 1 });

      // Wait for loadDis to complete
      await loadPromise;

      // After: loading should be false, items populated
      expect(useDiStore.getState().loading).toBe(false);
      expect(useDiStore.getState().items).toHaveLength(1);
      expect(useDiStore.getState().total).toBe(1);
    });

    it("loading=false and error set when loadDis rejects", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("network error"));

      await useDiStore.getState().loadDis();

      expect(useDiStore.getState().loading).toBe(false);
      expect(useDiStore.getState().error).toBeTruthy();
      expect(useDiStore.getState().items).toHaveLength(0);
    });

    it("loadDis uses the current filter from state", async () => {
      useDiStore.getState().setFilter({ status: ["pending_review"], limit: 25 });

      mockInvoke.mockResolvedValueOnce({ items: [], total: 0 });

      await useDiStore.getState().loadDis();

      expect(mockInvoke).toHaveBeenCalledWith("list_di", {
        filter: expect.objectContaining({
          status: ["pending_review"],
          limit: 25,
          offset: 0,
        }),
      });
    });
  });

  // ── openDi loads detail payload ───────────────────────────────────────────

  it("openDi sets activeDi with transitions and similar", async () => {
    const detail = {
      di: makeDi(),
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
      similar: [
        {
          id: 2,
          code: "DI-0002",
          title: "Similar issue",
          status: "submitted",
          submitted_at: "2026-03-30T08:00:00Z",
        },
      ],
    };
    mockInvoke.mockResolvedValueOnce(detail);

    await useDiStore.getState().openDi(1);

    const state = useDiStore.getState();
    expect(state.activeDi).not.toBeNull();
    expect(state.activeDi?.di.code).toBe("DI-0001");
    expect(state.activeDi?.transitions).toHaveLength(1);
    expect(state.activeDi?.similar).toHaveLength(1);
    expect(state.loading).toBe(false);
  });

  // ── submitNewDi reloads list ──────────────────────────────────────────────

  it("submitNewDi creates and reloads list", async () => {
    const newDi = makeDi({ id: 3, code: "DI-0003" });
    const pageAfter = { items: [makeDi(), newDi], total: 2 };

    mockInvoke
      .mockResolvedValueOnce(newDi) // create_di
      .mockResolvedValueOnce(pageAfter); // list_di (reload)

    const result = await useDiStore.getState().submitNewDi({
      asset_id: 10,
      org_node_id: 5,
      title: "New DI",
      description: "Desc",
      origin_type: "operator",
      impact_level: "minor",
      production_impact: false,
      safety_flag: false,
      environmental_flag: false,
      quality_flag: false,
      reported_urgency: "medium",
      submitter_id: 42,
    });

    expect(result.code).toBe("DI-0003");
    expect(useDiStore.getState().items).toHaveLength(2);
    expect(useDiStore.getState().saving).toBe(false);
  });
});

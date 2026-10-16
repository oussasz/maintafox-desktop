/**
 * Supervisor Verification — Phase 2 - SP04 - F01 - Sprint S3
 *
 * V3 — Store filter merge: setFilter merges partial updates into existing filter.
 * V4 — Loading state: loadDis sets loading=true while awaiting and loading=false after.
 */

import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { InterventionRequest } from "@shared/ipc-types";

import { DI_LIST_PAGE_SIZE, useDiStore } from "../di-store";

// ── Fixture ───────────────────────────────────────────────────────────────────

function makeDi(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 1,
    code: "DI-0001",
    equipment_id: 10,
    sub_asset_ref: null,
    org_node_id: 5,
    status: "submitted",
    title: "Fuite pompe",
    description: "Fuite détectée",
    origin_type: "operator",
    request_type: "repair",
    symptom_code_id: null,
    impact_level: "minor",
    production_impact: false,
    safety_flag: false,
    environmental_flag: false,
    quality_flag: false,
    reported_urgency: "normal",
    validated_urgency: null,
    observed_at: null,
    submitted_at: "2026-04-01T10:00:00Z",
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
    loadingMore: false,
    saving: false,
    error: null,
    listError: null,
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
    it("merges status filter then equipment_id filter", () => {
      const { setFilter } = useDiStore.getState();

      setFilter({ status: ["submitted"] });
      setFilter({ equipment_id: 5 });

      const { filter } = useDiStore.getState();
      expect(filter.status).toEqual(["submitted"]);
      expect(filter.equipment_id).toBe(5);
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
      useDiStore.getState().setFilter({ status: ["in_review"], limit: 25 });

      mockInvoke.mockResolvedValueOnce({ items: [], total: 0 });

      await useDiStore.getState().loadDis();

      expect(mockInvoke).toHaveBeenCalledWith("list_di", {
        filter: expect.objectContaining({
          status: ["in_review"],
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
      equipment_id: 10,
      org_node_id: 5,
      title: "New DI",
      description: "Desc",
      origin_type: "operator",
      request_type: "repair",
      impact_level: "minor",
      production_impact: false,
      safety_flag: false,
      environmental_flag: false,
      quality_flag: false,
      reported_urgency: "normal",
      submitter_id: 42,
    });

    expect(result.code).toBe("DI-0003");
    expect(useDiStore.getState().items).toHaveLength(2);
    expect(useDiStore.getState().saving).toBe(false);
  });
});

describe("di-store — paging and load errors", () => {
  it("loads pages of DI_LIST_PAGE_SIZE from the first record", async () => {
    useDiStore.setState({ filter: { limit: DI_LIST_PAGE_SIZE, offset: 400 } });
    mockInvoke.mockResolvedValueOnce({ items: [makeDi()], total: 1 });

    await useDiStore.getState().loadDis();

    const [, args] = mockInvoke.mock.calls[0] as [string, { filter: Record<string, unknown> }];
    expect(args.filter).toMatchObject({ limit: DI_LIST_PAGE_SIZE, offset: 0 });
  });

  it("appends the next page and skips records already loaded", async () => {
    useDiStore.setState({
      items: [makeDi({ id: 1 }), makeDi({ id: 2, code: "DI-0002" })],
      total: 3,
    });
    mockInvoke.mockResolvedValueOnce({
      items: [makeDi({ id: 2, code: "DI-0002" }), makeDi({ id: 3, code: "DI-0003" })],
      total: 3,
    });

    await useDiStore.getState().loadMoreDis();

    const [, args] = mockInvoke.mock.calls[0] as [string, { filter: Record<string, unknown> }];
    expect(args.filter).toMatchObject({ offset: 2 });
    expect(useDiStore.getState().items.map((d) => d.id)).toEqual([1, 2, 3]);
    expect(useDiStore.getState().loadingMore).toBe(false);
  });

  it("keeps the raw failure for the error state and stops loading", async () => {
    mockInvoke.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "disk I/O error" });

    await useDiStore.getState().loadDis();

    const s = useDiStore.getState();
    expect(s.listError).toEqual({ code: "DATABASE_ERROR", message: "disk I/O error" });
    expect(s.loading).toBe(false);
  });

  it("keeps the list of the latest load when an older load answers last", async () => {
    let resolveOlder!: (v: unknown) => void;
    mockInvoke
      .mockReturnValueOnce(new Promise((r) => (resolveOlder = r)))
      .mockResolvedValueOnce({ items: [makeDi({ id: 7, code: "DI-0007" })], total: 1 });

    const older = useDiStore.getState().loadDis();
    await useDiStore.getState().loadDis();
    resolveOlder({ items: [makeDi({ id: 1 }), makeDi({ id: 2, code: "DI-0002" })], total: 2 });
    await older;

    expect(useDiStore.getState().items.map((d) => d.id)).toEqual([7]);
  });
});

describe("di-store — work queues and the requester's answer", () => {
  const counts = {
    to_screen: 2,
    to_review: 1,
    awaiting_info: 1,
    to_approve: 0,
    approved_not_converted: 1,
    deferred_due: 0,
    sla_at_risk: 3,
    mine: 4,
    mine_awaiting_info: 1,
  };

  it("loads the queue sizes", async () => {
    mockInvoke.mockResolvedValueOnce(counts);
    await useDiStore.getState().loadQueueCounts();
    expect(useDiStore.getState().queueCounts).toEqual(counts);
    expect(useDiStore.getState().queueCountsError).toBeNull();
  });

  it("keeps the last sizes and the failure when the count cannot be read", async () => {
    useDiStore.setState({ queueCounts: counts, queueCountsError: null });
    mockInvoke.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "locked" });
    await useDiStore.getState().loadQueueCounts();
    expect(useDiStore.getState().queueCounts).toEqual(counts);
    expect(useDiStore.getState().queueCountsError).toEqual({
      code: "DATABASE_ERROR",
      message: "locked",
    });
  });

  it("resubmits, then refreshes the list, the detail and the queue sizes", async () => {
    const back = makeDi({ status: "in_review", row_version: 3 });
    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "resubmit_di":
          return Promise.resolve(back);
        case "list_di":
          return Promise.resolve({ items: [back], total: 1 });
        case "get_di":
          return Promise.resolve({ di: back, transitions: [], similar: [] });
        case "get_di_queue_counts":
          return Promise.resolve(counts);
        default:
          return Promise.reject(new Error(`unexpected ${cmd}`));
      }
    });

    const di = await useDiStore.getState().resubmit({
      di_id: 1,
      actor_id: 0,
      expected_row_version: 2,
      response_note: "Leak is on the outlet flange",
    });

    expect(di.status).toBe("in_review");
    const s = useDiStore.getState();
    expect(s.items[0]?.status).toBe("in_review");
    expect(s.activeDi?.di.row_version).toBe(3);
    expect(s.saving).toBe(false);
    expect(mockInvoke).toHaveBeenCalledWith("resubmit_di", {
      input: {
        di_id: 1,
        actor_id: 0,
        expected_row_version: 2,
        response_note: "Leak is on the outlet flange",
      },
    });
  });

  it("reports a refused resubmission and rethrows it", async () => {
    mockInvoke.mockRejectedValueOnce({ code: "VALIDATION_FAILED", message: "answer required" });
    await expect(
      useDiStore.getState().resubmit({
        di_id: 1,
        actor_id: 0,
        expected_row_version: 2,
        response_note: " ",
      }),
    ).rejects.toBeTruthy();
    expect(useDiStore.getState().error).toBeTruthy();
    expect(useDiStore.getState().saving).toBe(false);
  });
});

/**
 * Behaviour tests for wo-store (work order list, detail, form state, lifecycle actions).
 * Only the wo-service / wo-closeout-service boundary is mocked; the real
 * CompletionBlockedError class is kept so `instanceof` checks behave as in prod.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { closeWo } from "@/services/wo-closeout-service";
import {
  CompletionBlockedError,
  assignWo,
  cancelWo,
  completeWoMechanically,
  createWo,
  getWo,
  listWos,
  pauseWo,
  planWo,
  resumeWo,
  startWo,
  updateWoDraft,
} from "@/services/wo-service";
import type * as WoService from "@/services/wo-service";
import type {
  AssetSearchResult,
  WoAssignInput,
  WoCancelInput,
  WoCloseInput,
  WoCreateInput,
  WoDraftUpdateInput,
  WoGetResponse,
  WoListPage,
  WoMechCompleteInput,
  WoMechCompleteResponse,
  WoPauseInput,
  WoPlanInput,
  WoPreflightError,
  WoResumeInput,
  WoStartInput,
  WoTransitionRow,
  WorkOrder,
} from "@shared/ipc-types";

import { WO_LIST_PAGE_SIZE, useWoStore } from "../wo-store";

vi.mock("@/services/wo-service", async (importOriginal) => {
  const actual = await importOriginal<typeof WoService>();
  return {
    CompletionBlockedError: actual.CompletionBlockedError,
    assignWo: vi.fn(),
    cancelWo: vi.fn(),
    completeWoMechanically: vi.fn(),
    createWo: vi.fn(),
    getWo: vi.fn(),
    listWos: vi.fn(),
    pauseWo: vi.fn(),
    planWo: vi.fn(),
    resumeWo: vi.fn(),
    startWo: vi.fn(),
    updateWoDraft: vi.fn(),
  };
});

vi.mock("@/services/wo-closeout-service", () => ({
  closeWo: vi.fn(),
}));

const TS = "2026-10-01T08:00:00Z";

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeWo(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: 1,
    code: "WO-0001",
    type_id: 1,
    status_id: 1,
    equipment_id: 10,
    component_id: null,
    location_id: null,
    requester_id: null,
    source_di_id: null,
    entity_id: null,
    planner_id: null,
    approver_id: null,
    assigned_team_id: null,
    primary_responsible_id: null,
    urgency_id: null,
    title: "Replace pump seal",
    description: null,
    planned_start: null,
    planned_end: null,
    shift: null,
    scheduled_at: null,
    actual_start: null,
    actual_end: null,
    mechanically_completed_at: null,
    technically_verified_at: null,
    closed_at: null,
    cancelled_at: null,
    expected_duration_hours: null,
    actual_duration_hours: null,
    active_labor_hours: null,
    total_waiting_hours: null,
    downtime_hours: null,
    planned_downtime_hours: null,
    labor_cost: null,
    parts_cost: null,
    service_cost: null,
    total_cost: null,
    recurrence_risk_level: null,
    production_impact_id: null,
    root_cause_summary: null,
    corrective_action_summary: null,
    verification_method: null,
    notes: null,
    cancel_reason: null,
    parts_actuals_confirmed: false,
    service_cost_input: null,
    reopen_count: 0,
    last_closed_at: null,
    requires_permit: false,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    status_code: "draft",
    ...overrides,
  };
}

function makeTransition(overrides: Partial<WoTransitionRow> = {}): WoTransitionRow {
  return {
    id: 100,
    wo_id: 1,
    from_status: "draft",
    to_status: "planned",
    action: "plan",
    actor_id: 1,
    reason_code: null,
    notes: null,
    acted_at: TS,
    ...overrides,
  };
}

function makeAsset(overrides: Partial<AssetSearchResult> = {}): AssetSearchResult {
  return {
    id: 10,
    entity_sync_id: "asset-10",
    asset_code: "P-101",
    asset_name: "Pump 101",
    class_code: null,
    class_name: null,
    family_code: null,
    family_name: null,
    subfamily_code: null,
    subfamily_name: null,
    criticality_code: null,
    status_code: "in_service",
    org_node_id: null,
    org_node_name: null,
    parent_asset_id: null,
    parent_asset_code: null,
    parent_asset_name: null,
    primary_meter_name: null,
    primary_meter_reading: null,
    primary_meter_unit: null,
    primary_meter_last_read_at: null,
    external_id_count: 0,
    row_version: 1,
    ...overrides,
  };
}

function page(items: WorkOrder[]): WoListPage {
  return { items, total: items.length };
}

function detail(wo: WorkOrder, transitions: WoTransitionRow[] = []): WoGetResponse {
  return { wo, transitions };
}

function deferred<T>() {
  let resolve: (v: T) => void = () => undefined;
  let reject: (e: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Let fire-and-forget follow-ups (`void get().loadWos()`) settle. */
async function flush() {
  await new Promise((r) => setTimeout(r, 0));
}

const base = { wo_id: 1, actor_id: 9, expected_row_version: 1 };
const createInput: WoCreateInput = { type_code: "CORR", title: "New WO", creator_id: 9 };
const draftInput: WoDraftUpdateInput = { id: 1, expected_row_version: 1, title: "Edited" };
const cancelInput: WoCancelInput = {
  id: 1,
  expected_row_version: 1,
  actor_id: 9,
  cancel_reason: "duplicate",
};
const planInput: WoPlanInput = { ...base, planner_id: 4 };
const assignInput: WoAssignInput = { ...base, assigned_team_id: 3, override_reason: "urgent" };
const startInput: WoStartInput = { ...base };
const pauseInput: WoPauseInput = { ...base, delay_reason_id: 2, comment: "waiting parts" };
const resumeInput: WoResumeInput = { ...base };
const completeInput: WoMechCompleteInput = { ...base, conclusion: "done" };
const closeInput: WoCloseInput = { ...base };

// ── Setup ─────────────────────────────────────────────────────────────────────

const initial = useWoStore.getState();

beforeEach(() => {
  vi.resetAllMocks();
  // Default for fire-and-forget list refreshes.
  vi.mocked(listWos).mockResolvedValue(page([]));
  useWoStore.setState(initial, true);
});

describe("wo-store", () => {
  it("starts with default filter and closed dialogs", () => {
    const s = useWoStore.getState();
    expect(s.items).toEqual([]);
    expect(s.total).toBe(0);
    expect(s.activeWo).toBeNull();
    expect(s.detailLoading).toBe(false);
    expect(s.openingWoId).toBeNull();
    expect(s.showCreateForm).toBe(false);
    expect(s.editingWo).toBeNull();
    expect(s.createPrefillAsset).toBeNull();
    expect(s.createPrefillEquipmentId).toBeNull();
    expect(s.showCompletionDialog).toBe(false);
    expect(s.completionErrors).toEqual([]);
    expect(s.filter).toEqual({ limit: WO_LIST_PAGE_SIZE, offset: 0 });
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
  });

  // ── Filter & list ─────────────────────────────────────────────────────────

  describe("setFilter", () => {
    it("merges partial updates and keeps pagination defaults", () => {
      const { setFilter } = useWoStore.getState();
      setFilter({ status_codes: ["draft"] });
      setFilter({ search: "pump", offset: 50 });
      expect(useWoStore.getState().filter).toEqual({
        limit: WO_LIST_PAGE_SIZE,
        offset: 50,
        status_codes: ["draft"],
        search: "pump",
      });
    });

    it("does not mutate the shared default filter object across resets", () => {
      useWoStore.getState().setFilter({ offset: 100 });
      useWoStore.setState(initial, true);
      expect(useWoStore.getState().filter).toEqual({ limit: WO_LIST_PAGE_SIZE, offset: 0 });
    });
  });

  describe("loadWos", () => {
    it("toggles loading and stores the page, passing the current filter", async () => {
      const d = deferred<WoListPage>();
      vi.mocked(listWos).mockReturnValueOnce(d.promise);
      useWoStore.getState().setFilter({ search: "seal" });
      useWoStore.setState({ error: "old" });

      const p = useWoStore.getState().loadWos();
      expect(useWoStore.getState().loading).toBe(true);
      expect(useWoStore.getState().error).toBeNull();

      d.resolve({ items: [makeWo(), makeWo({ id: 2 })], total: 7 });
      await p;

      expect(listWos).toHaveBeenCalledWith({ limit: WO_LIST_PAGE_SIZE, offset: 0, search: "seal" });
      const s = useWoStore.getState();
      expect(s.items).toHaveLength(2);
      expect(s.total).toBe(7);
      expect(s.loading).toBe(false);
    });

    it("swallows errors, sets the message and keeps previous items", async () => {
      const previous = [makeWo()];
      useWoStore.setState({ items: previous, total: 1 });
      vi.mocked(listWos).mockRejectedValueOnce({ code: "DB", message: "list failed" });

      await expect(useWoStore.getState().loadWos()).resolves.toBeUndefined();

      const s = useWoStore.getState();
      expect(s.error).toBe("list failed");
      expect(s.listError).toEqual({ code: "DB", message: "list failed" });
      expect(s.items).toBe(previous);
      expect(s.total).toBe(1);
      expect(s.loading).toBe(false);
    });

    it("always reloads from the first page, whatever offset the filter holds", async () => {
      useWoStore.getState().setFilter({ offset: 400 });
      await useWoStore.getState().loadWos();
      expect(listWos).toHaveBeenCalledWith({ limit: WO_LIST_PAGE_SIZE, offset: 0 });
    });
  });

  describe("loadMoreWos", () => {
    it("appends the next page after the items already loaded, skipping duplicates", async () => {
      useWoStore.setState({ items: [makeWo({ id: 1 }), makeWo({ id: 2 })], total: 4 });
      vi.mocked(listWos).mockResolvedValueOnce({
        items: [makeWo({ id: 2 }), makeWo({ id: 3 }), makeWo({ id: 4 })],
        total: 4,
      });

      await useWoStore.getState().loadMoreWos();

      expect(listWos).toHaveBeenCalledWith({ limit: WO_LIST_PAGE_SIZE, offset: 2 });
      const s = useWoStore.getState();
      expect(s.items.map((w) => w.id)).toEqual([1, 2, 3, 4]);
      expect(s.loadingMore).toBe(false);
    });

    it("does nothing when every record is already loaded", async () => {
      useWoStore.setState({ items: [makeWo({ id: 1 })], total: 1 });
      await useWoStore.getState().loadMoreWos();
      expect(listWos).not.toHaveBeenCalled();
    });

    it("keeps the loaded pages when the next page fails", async () => {
      const loaded = [makeWo({ id: 1 })];
      useWoStore.setState({ items: loaded, total: 3 });
      vi.mocked(listWos).mockRejectedValueOnce({ code: "DB", message: "boom" });

      await useWoStore.getState().loadMoreWos();

      const s = useWoStore.getState();
      expect(s.items).toBe(loaded);
      expect(s.loadingMore).toBe(false);
    });

    it("keeps the list of the latest filter when an older list answers last", async () => {
      const older = deferred<WoListPage>();
      const newer = deferred<WoListPage>();
      vi.mocked(listWos).mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);

      const p1 = useWoStore.getState().loadWos();
      useWoStore.getState().setFilter({ search: "pump" });
      const p2 = useWoStore.getState().loadWos();
      newer.resolve({ items: [makeWo({ id: 2 })], total: 1 });
      await p2;
      expect(useWoStore.getState().loading).toBe(false);
      older.resolve({ items: [makeWo({ id: 1 }), makeWo({ id: 3 })], total: 2 });
      await p1;

      const s = useWoStore.getState();
      expect(s.items.map((w) => w.id)).toEqual([2]);
      expect(s.total).toBe(1);
    });
  });

  // ── Detail ────────────────────────────────────────────────────────────────

  describe("openWo", () => {
    it("clears a different active WO while loading, then sets the new detail", async () => {
      useWoStore.setState({ activeWo: { wo: makeWo({ id: 5 }), transitions: [] } });
      const d = deferred<WoGetResponse>();
      vi.mocked(getWo).mockReturnValueOnce(d.promise);

      const p = useWoStore.getState().openWo(1);
      let s = useWoStore.getState();
      expect(s.detailLoading).toBe(true);
      expect(s.openingWoId).toBe(1);
      expect(s.activeWo).toBeNull();

      const resp = detail(makeWo(), [makeTransition()]);
      d.resolve(resp);
      await p;

      expect(getWo).toHaveBeenCalledWith(1);
      s = useWoStore.getState();
      expect(s.activeWo).toEqual({ wo: resp.wo, transitions: resp.transitions });
      expect(s.detailLoading).toBe(false);
      expect(s.openingWoId).toBeNull();
    });

    it("keeps the current detail visible while re-opening the same WO", async () => {
      const current = { wo: makeWo(), transitions: [makeTransition()] };
      useWoStore.setState({ activeWo: current });
      const d = deferred<WoGetResponse>();
      vi.mocked(getWo).mockReturnValueOnce(d.promise);

      const p = useWoStore.getState().openWo(1);
      expect(useWoStore.getState().activeWo).toBe(current);

      d.resolve(detail(makeWo({ row_version: 2 })));
      await p;
      expect(useWoStore.getState().activeWo?.wo.row_version).toBe(2);
    });

    it("on failure for a different WO: sets error and leaves no active WO", async () => {
      useWoStore.setState({ activeWo: { wo: makeWo({ id: 5 }), transitions: [] } });
      vi.mocked(getWo).mockRejectedValueOnce(new Error("not found"));

      await expect(useWoStore.getState().openWo(1)).resolves.toBeUndefined();

      const s = useWoStore.getState();
      expect(s.error).toBe("not found");
      expect(s.activeWo).toBeNull();
      expect(s.detailLoading).toBe(false);
      expect(s.openingWoId).toBeNull();
    });

    it("on failure for the same WO: keeps the current detail", async () => {
      const current = { wo: makeWo(), transitions: [] };
      useWoStore.setState({ activeWo: current });
      vi.mocked(getWo).mockRejectedValueOnce(new Error("timeout"));

      await useWoStore.getState().openWo(1);

      expect(useWoStore.getState().activeWo).toBe(current);
      expect(useWoStore.getState().error).toBe("timeout");
    });

    it("a slow openWo response for an earlier WO never overwrites the later selection", async () => {
      const first = deferred<WoGetResponse>();
      const second = deferred<WoGetResponse>();
      vi.mocked(getWo).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

      const p1 = useWoStore.getState().openWo(1);
      const p2 = useWoStore.getState().openWo(2);
      second.resolve(detail(makeWo({ id: 2 })));
      await p2;
      first.resolve(detail(makeWo({ id: 1 })));
      await p1;

      const s = useWoStore.getState();
      expect(s.activeWo?.wo.id).toBe(2);
      expect(s.detailLoading).toBe(false);
    });

    it("a late failure of an earlier openWo does not clobber the later detail", async () => {
      const first = deferred<WoGetResponse>();
      vi.mocked(getWo)
        .mockReturnValueOnce(first.promise)
        .mockResolvedValueOnce(detail(makeWo({ id: 2 })));

      const p1 = useWoStore.getState().openWo(1);
      await useWoStore.getState().openWo(2);
      first.reject(new Error("timeout"));
      await p1;

      expect(useWoStore.getState().activeWo?.wo.id).toBe(2);
      expect(useWoStore.getState().error).toBeNull();
    });

    it("a detail that arrives after the dialog was closed is dropped", async () => {
      const first = deferred<WoGetResponse>();
      vi.mocked(getWo).mockReturnValueOnce(first.promise);
      const p = useWoStore.getState().openWo(1);
      useWoStore.getState().closeActiveWo();
      first.resolve(detail(makeWo({ id: 1 })));
      await p;
      expect(useWoStore.getState().activeWo).toBeNull();
    });
  });

  it("closeActiveWo clears detail and loading markers", () => {
    useWoStore.setState({
      activeWo: { wo: makeWo(), transitions: [] },
      detailLoading: true,
      openingWoId: 1,
    });
    useWoStore.getState().closeActiveWo();
    const s = useWoStore.getState();
    expect(s.activeWo).toBeNull();
    expect(s.detailLoading).toBe(false);
    expect(s.openingWoId).toBeNull();
  });

  describe("refreshActiveWo", () => {
    it("does nothing without an active WO", async () => {
      await useWoStore.getState().refreshActiveWo();
      expect(getWo).not.toHaveBeenCalled();
    });

    it("re-fetches the active WO and replaces detail + transitions", async () => {
      useWoStore.setState({ activeWo: { wo: makeWo({ id: 3 }), transitions: [] } });
      const resp = detail(makeWo({ id: 3, row_version: 4 }), [makeTransition({ wo_id: 3 })]);
      vi.mocked(getWo).mockResolvedValueOnce(resp);

      await useWoStore.getState().refreshActiveWo();

      expect(getWo).toHaveBeenCalledWith(3);
      expect(useWoStore.getState().activeWo).toEqual(resp);
    });

    it("silently ignores errors (no error message, detail kept)", async () => {
      const current = { wo: makeWo(), transitions: [] };
      useWoStore.setState({ activeWo: current });
      vi.mocked(getWo).mockRejectedValueOnce(new Error("deleted"));

      await expect(useWoStore.getState().refreshActiveWo()).resolves.toBeUndefined();

      expect(useWoStore.getState().activeWo).toBe(current);
      expect(useWoStore.getState().error).toBeNull();
    });
  });

  // ── Create / edit form ────────────────────────────────────────────────────

  describe("openCreateForm / closeCreateForm", () => {
    it("opens a blank create form", () => {
      useWoStore.getState().openCreateForm();
      const s = useWoStore.getState();
      expect(s.showCreateForm).toBe(true);
      expect(s.editingWo).toBeNull();
      expect(s.createPrefillAsset).toBeNull();
      expect(s.createPrefillEquipmentId).toBeNull();
    });

    it("prefills the asset and equipment id for a new WO", () => {
      const asset = makeAsset({ id: 42 });
      useWoStore.getState().openCreateForm(undefined, asset);
      const s = useWoStore.getState();
      expect(s.createPrefillAsset).toBe(asset);
      expect(s.createPrefillEquipmentId).toBe(42);
    });

    it("ignores the prefill asset when editing an existing WO", () => {
      const wo = makeWo();
      useWoStore.getState().openCreateForm(wo, makeAsset());
      const s = useWoStore.getState();
      expect(s.editingWo).toBe(wo);
      expect(s.createPrefillAsset).toBeNull();
      expect(s.createPrefillEquipmentId).toBeNull();
    });

    it("closeCreateForm resets all form state", () => {
      useWoStore.getState().openCreateForm(undefined, makeAsset());
      useWoStore.getState().closeCreateForm();
      const s = useWoStore.getState();
      expect(s.showCreateForm).toBe(false);
      expect(s.editingWo).toBeNull();
      expect(s.createPrefillAsset).toBeNull();
      expect(s.createPrefillEquipmentId).toBeNull();
    });
  });

  describe("submitNewWo", () => {
    it("creates the WO, closes the form, refreshes the list and returns the WO", async () => {
      useWoStore.getState().openCreateForm(undefined, makeAsset());
      const created = makeWo({ id: 11 });
      const d = deferred<WorkOrder>();
      vi.mocked(createWo).mockReturnValueOnce(d.promise);
      vi.mocked(listWos).mockResolvedValueOnce(page([created]));

      const p = useWoStore.getState().submitNewWo(createInput);
      expect(useWoStore.getState().saving).toBe(true);
      d.resolve(created);

      await expect(p).resolves.toBe(created);
      await flush();

      expect(createWo).toHaveBeenCalledWith(createInput);
      const s = useWoStore.getState();
      expect(s.saving).toBe(false);
      expect(s.showCreateForm).toBe(false);
      expect(s.createPrefillAsset).toBeNull();
      expect(s.createPrefillEquipmentId).toBeNull();
      expect(listWos).toHaveBeenCalledTimes(1);
      expect(s.items).toEqual([created]);
    });

    it("sets error, rethrows and keeps the form open on failure", async () => {
      useWoStore.getState().openCreateForm();
      const err = { code: "VALIDATION", message: "title required" };
      vi.mocked(createWo).mockRejectedValueOnce(err);

      await expect(useWoStore.getState().submitNewWo(createInput)).rejects.toBe(err);

      const s = useWoStore.getState();
      expect(s.error).toBe("title required");
      expect(s.saving).toBe(false);
      expect(s.showCreateForm).toBe(true);
      expect(listWos).not.toHaveBeenCalled();
    });
  });

  describe("updateDraft", () => {
    it("replaces the active WO but keeps its transitions, closes the form and refreshes", async () => {
      const transitions = [makeTransition()];
      useWoStore.setState({ activeWo: { wo: makeWo(), transitions } });
      useWoStore.getState().openCreateForm(makeWo());
      const updated = makeWo({ title: "Edited", row_version: 2 });
      vi.mocked(updateWoDraft).mockResolvedValueOnce(updated);

      await useWoStore.getState().updateDraft(draftInput);
      await flush();

      expect(updateWoDraft).toHaveBeenCalledWith(draftInput);
      const s = useWoStore.getState();
      expect(s.activeWo).toEqual({ wo: updated, transitions });
      expect(s.showCreateForm).toBe(false);
      expect(s.editingWo).toBeNull();
      expect(s.saving).toBe(false);
      expect(listWos).toHaveBeenCalledTimes(1);
    });

    it("uses empty transitions when there was no active WO", async () => {
      const updated = makeWo({ row_version: 2 });
      vi.mocked(updateWoDraft).mockResolvedValueOnce(updated);

      await useWoStore.getState().updateDraft(draftInput);

      expect(useWoStore.getState().activeWo).toEqual({ wo: updated, transitions: [] });
    });

    it("sets error and rethrows on failure, keeping the edit form", async () => {
      const wo = makeWo();
      useWoStore.getState().openCreateForm(wo);
      vi.mocked(updateWoDraft).mockRejectedValueOnce(new Error("version conflict"));

      await expect(useWoStore.getState().updateDraft(draftInput)).rejects.toThrow(
        "version conflict",
      );

      const s = useWoStore.getState();
      expect(s.error).toBe("version conflict");
      expect(s.editingWo).toBe(wo);
      expect(s.saving).toBe(false);
    });
  });

  describe("cancel", () => {
    it("cancels then refreshes the active WO detail and the list", async () => {
      useWoStore.setState({ activeWo: { wo: makeWo(), transitions: [] } });
      vi.mocked(cancelWo).mockResolvedValueOnce(makeWo({ status_code: "cancelled" }));
      const refreshed = detail(makeWo({ status_code: "cancelled", row_version: 2 }), [
        makeTransition({ to_status: "cancelled", action: "cancel" }),
      ]);
      vi.mocked(getWo).mockResolvedValueOnce(refreshed);

      await useWoStore.getState().cancel(cancelInput);
      await flush();

      expect(cancelWo).toHaveBeenCalledWith(cancelInput);
      expect(getWo).toHaveBeenCalledWith(1);
      expect(listWos).toHaveBeenCalledTimes(1);
      expect(useWoStore.getState().activeWo).toEqual(refreshed);
      expect(useWoStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure without refreshing", async () => {
      vi.mocked(cancelWo).mockRejectedValueOnce(new Error("cannot cancel"));

      await expect(useWoStore.getState().cancel(cancelInput)).rejects.toThrow("cannot cancel");

      expect(useWoStore.getState().error).toBe("cannot cancel");
      expect(useWoStore.getState().saving).toBe(false);
      expect(listWos).not.toHaveBeenCalled();
    });
  });

  // ── Transition actions sharing the same shape ─────────────────────────────

  describe.each([
    {
      name: "planWorkOrder",
      svc: planWo,
      input: planInput,
      run: () => useWoStore.getState().planWorkOrder(planInput),
    },
    {
      name: "assignWorkOrder",
      svc: assignWo,
      input: assignInput,
      run: () => useWoStore.getState().assignWorkOrder(assignInput),
    },
    {
      name: "startWorkOrder",
      svc: startWo,
      input: startInput,
      run: () => useWoStore.getState().startWorkOrder(startInput),
    },
    {
      name: "pauseWorkOrder",
      svc: pauseWo,
      input: pauseInput,
      run: () => useWoStore.getState().pauseWorkOrder(pauseInput),
    },
    {
      name: "resumeWorkOrder",
      svc: resumeWo,
      input: resumeInput,
      run: () => useWoStore.getState().resumeWorkOrder(resumeInput),
    },
    {
      name: "closeWorkOrder",
      svc: closeWo,
      input: closeInput,
      run: () => useWoStore.getState().closeWorkOrder(closeInput),
    },
    {
      name: "cancelWorkOrder",
      svc: cancelWo,
      input: cancelInput,
      run: () => useWoStore.getState().cancelWorkOrder(cancelInput),
    },
  ])("$name", ({ svc, input, run }) => {
    const mocked = () => vi.mocked(svc as (i: unknown) => Promise<WorkOrder>);

    it("toggles saving, swaps the WO in activeWo keeping transitions, and refreshes the list", async () => {
      const transitions = [makeTransition()];
      useWoStore.setState({ activeWo: { wo: makeWo(), transitions }, error: "old" });
      const next = makeWo({ row_version: 2, status_code: "next" });
      const d = deferred<WorkOrder>();
      mocked().mockReturnValueOnce(d.promise);

      const p = run();
      expect(useWoStore.getState().saving).toBe(true);
      expect(useWoStore.getState().error).toBeNull();
      d.resolve(next);
      await p;
      await flush();

      expect(mocked()).toHaveBeenCalledWith(input);
      const s = useWoStore.getState();
      expect(s.activeWo).toEqual({ wo: next, transitions });
      expect(s.saving).toBe(false);
      expect(listWos).toHaveBeenCalledTimes(1);
    });

    it("sets an active WO with empty transitions when none was open", async () => {
      const next = makeWo({ row_version: 2 });
      mocked().mockResolvedValueOnce(next);
      await run();
      expect(useWoStore.getState().activeWo).toEqual({ wo: next, transitions: [] });
    });

    it("sets error and rethrows on failure, leaving activeWo untouched", async () => {
      const current = { wo: makeWo(), transitions: [] };
      useWoStore.setState({ activeWo: current });
      const err = { code: "INVALID_TRANSITION", message: "not allowed" };
      mocked().mockRejectedValueOnce(err);

      await expect(run()).rejects.toBe(err);

      const s = useWoStore.getState();
      expect(s.error).toBe("not allowed");
      expect(s.activeWo).toBe(current);
      expect(s.saving).toBe(false);
      expect(listWos).not.toHaveBeenCalled();
    });
  });

  // ── Completion dialog ─────────────────────────────────────────────────────

  describe("completion dialog", () => {
    it("openCompletionDialog shows the dialog and clears previous errors", () => {
      useWoStore.setState({ completionErrors: [{ code: "X", message: "x" }] });
      useWoStore.getState().openCompletionDialog();
      expect(useWoStore.getState().showCompletionDialog).toBe(true);
      expect(useWoStore.getState().completionErrors).toEqual([]);
    });

    it("closeCompletionDialog hides the dialog and clears errors", () => {
      useWoStore.setState({
        showCompletionDialog: true,
        completionErrors: [{ code: "X", message: "x" }],
      });
      useWoStore.getState().closeCompletionDialog();
      expect(useWoStore.getState().showCompletionDialog).toBe(false);
      expect(useWoStore.getState().completionErrors).toEqual([]);
    });
  });

  describe("completeWorkOrder", () => {
    it("on clean completion: updates the WO, closes the dialog and refreshes the list", async () => {
      const transitions = [makeTransition()];
      useWoStore.setState({
        activeWo: { wo: makeWo(), transitions },
        showCompletionDialog: true,
      });
      const resp: WoMechCompleteResponse = {
        wo: makeWo({ status_code: "mechanically_complete", row_version: 2 }),
        errors: [],
      };
      const d = deferred<WoMechCompleteResponse>();
      vi.mocked(completeWoMechanically).mockReturnValueOnce(d.promise);

      const p = useWoStore.getState().completeWorkOrder(completeInput);
      expect(useWoStore.getState().saving).toBe(true);
      d.resolve(resp);

      await expect(p).resolves.toBe(resp);
      await flush();

      expect(completeWoMechanically).toHaveBeenCalledWith(completeInput);
      const s = useWoStore.getState();
      expect(s.activeWo).toEqual({ wo: resp.wo, transitions });
      expect(s.completionErrors).toEqual([]);
      expect(s.showCompletionDialog).toBe(false);
      expect(s.saving).toBe(false);
      expect(listWos).toHaveBeenCalledTimes(1);
    });

    it("when the response carries errors: keeps the dialog open and exposes them", async () => {
      useWoStore.setState({ showCompletionDialog: true });
      const errors: WoPreflightError[] = [{ code: "OPEN_LABOR", message: "open labor" }];
      const resp: WoMechCompleteResponse = { wo: makeWo(), errors };
      vi.mocked(completeWoMechanically).mockResolvedValueOnce(resp);

      await expect(useWoStore.getState().completeWorkOrder(completeInput)).resolves.toBe(resp);

      const s = useWoStore.getState();
      expect(s.completionErrors).toEqual(errors);
      expect(s.showCompletionDialog).toBe(true);
      expect(s.activeWo).toEqual({ wo: resp.wo, transitions: [] });
    });

    it("on CompletionBlockedError: resolves with the blockers, opens the dialog, no error message", async () => {
      const wo = makeWo();
      useWoStore.setState({ activeWo: { wo, transitions: [] } });
      const errors: WoPreflightError[] = [
        { code: "INCOMPLETE_TASKS", message: "mandatory tasks incomplete" },
      ];
      vi.mocked(completeWoMechanically).mockRejectedValueOnce(new CompletionBlockedError(errors));

      const result = await useWoStore.getState().completeWorkOrder(completeInput);

      expect(result).toEqual({ wo, errors });
      const s = useWoStore.getState();
      expect(s.completionErrors).toEqual(errors);
      expect(s.showCompletionDialog).toBe(true);
      expect(s.error).toBeNull();
      expect(s.saving).toBe(false);
      expect(listWos).not.toHaveBeenCalled();
    });

    it("on CompletionBlockedError without an active WO: result.wo is undefined despite the WorkOrder type", async () => {
      // Documents a type hole: the store casts `{ wo: undefined, errors }` to
      // WoMechCompleteResponse, so callers reading `result.wo.*` can crash.
      vi.mocked(completeWoMechanically).mockRejectedValueOnce(
        new CompletionBlockedError([{ code: "MISSING_PARTS", message: "parts" }]),
      );

      const result = await useWoStore.getState().completeWorkOrder(completeInput);

      expect(result.wo).toBeUndefined();
    });

    it("on any other error: sets error and rethrows", async () => {
      vi.mocked(completeWoMechanically).mockRejectedValueOnce(new Error("version conflict"));

      await expect(useWoStore.getState().completeWorkOrder(completeInput)).rejects.toThrow(
        "version conflict",
      );

      const s = useWoStore.getState();
      expect(s.error).toBe("version conflict");
      expect(s.showCompletionDialog).toBe(false);
      expect(s.saving).toBe(false);
    });
  });
});

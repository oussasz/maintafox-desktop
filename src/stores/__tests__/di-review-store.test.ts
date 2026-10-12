/**
 * Behaviour tests for the DI review store (review queue, review context, triage actions).
 * Only the DI and DI review services (IPC boundary) are mocked.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  approveDi,
  archiveDi,
  cancelOwnDi,
  closeDi,
  closeDiAsNonExecutable,
  deferDi,
  getDiReviewEvents,
  reactivateDi,
  returnDi,
  screenDi,
} from "@/services/di-review-service";
import { getDi, listDis } from "@/services/di-service";
import type {
  DiGetResponse,
  DiListPage,
  DiReviewEvent,
  DiSummaryRow,
  InterventionRequest,
} from "@shared/ipc-types";

import { useDiReviewStore } from "../di-review-store";

vi.mock("@/services/di-review-service", () => ({
  approveDi: vi.fn(),
  archiveDi: vi.fn(),
  cancelOwnDi: vi.fn(),
  closeDi: vi.fn(),
  closeDiAsNonExecutable: vi.fn(),
  deferDi: vi.fn(),
  getDiReviewEvents: vi.fn(),
  reactivateDi: vi.fn(),
  returnDi: vi.fn(),
  screenDi: vi.fn(),
}));

vi.mock("@/services/di-service", () => ({
  getDi: vi.fn(),
  listDis: vi.fn(),
}));

const mockListDis = vi.mocked(listDis);
const mockGetDi = vi.mocked(getDi);
const mockGetDiReviewEvents = vi.mocked(getDiReviewEvents);
const mockScreenDi = vi.mocked(screenDi);
const mockReturnDi = vi.mocked(returnDi);
const mockCloseDi = vi.mocked(closeDi);
const mockCancelOwnDi = vi.mocked(cancelOwnDi);
const mockApproveDi = vi.mocked(approveDi);
const mockDeferDi = vi.mocked(deferDi);
const mockReactivateDi = vi.mocked(reactivateDi);
const mockCloseNonExec = vi.mocked(closeDiAsNonExecutable);
const mockArchiveDi = vi.mocked(archiveDi);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeDi(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 1,
    code: "DI-0001",
    equipment_id: 10,
    sub_asset_ref: null,
    org_node_id: 5,
    status: "in_review",
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
    reported_urgency: "medium",
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
    is_modified: false,
    row_version: 1,
    submitter_id: 42,
    created_at: "2026-04-01T10:00:00Z",
    updated_at: "2026-04-01T10:00:00Z",
    ...overrides,
  };
}

function makePage(items: InterventionRequest[]): DiListPage {
  return { items, total: items.length };
}

function makeEvent(overrides: Partial<DiReviewEvent> = {}): DiReviewEvent {
  return {
    id: 1,
    di_id: 1,
    event_type: "screened",
    actor_id: 7,
    acted_at: "2026-04-02T10:00:00Z",
    from_status: "submitted",
    to_status: "in_review",
    reason_code: null,
    notes: null,
    sla_target_hours: null,
    sla_deadline: null,
    step_up_used: false,
    ...overrides,
  };
}

function makeSummary(overrides: Partial<DiSummaryRow> = {}): DiSummaryRow {
  return {
    id: 2,
    code: "DI-0002",
    title: "Fuite similaire",
    status: "closed",
    submitted_at: "2026-03-01T10:00:00Z",
    ...overrides,
  };
}

function makeDetail(di: InterventionRequest, similar: DiSummaryRow[] = []): DiGetResponse {
  return { di, transitions: [], similar };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const QUEUE_FILTER = {
  status: ["in_review", "returned_for_clarification", "awaiting_approval"],
  limit: 200,
  offset: 0,
};

const initial = useDiReviewStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  useDiReviewStore.setState(initial, true);
  mockListDis.mockResolvedValue(makePage([]));
});

// ── loadReviewQueue ───────────────────────────────────────────────────────────

describe("loadReviewQueue", () => {
  it("requests the review statuses and stores the items", async () => {
    const items = [makeDi(), makeDi({ id: 2, status: "awaiting_approval" })];
    mockListDis.mockResolvedValueOnce(makePage(items));
    useDiReviewStore.setState({ error: "old" });

    await useDiReviewStore.getState().loadReviewQueue();

    expect(mockListDis).toHaveBeenCalledWith(QUEUE_FILTER);
    expect(useDiReviewStore.getState().reviewQueue).toEqual(items);
    expect(useDiReviewStore.getState().error).toBeNull();
  });

  it("swallows a failure into error and keeps the previous queue", async () => {
    const prev = [makeDi()];
    useDiReviewStore.setState({ reviewQueue: prev });
    mockListDis.mockRejectedValueOnce(new Error("offline"));

    await expect(useDiReviewStore.getState().loadReviewQueue()).resolves.toBeUndefined();
    expect(useDiReviewStore.getState().error).toBe("offline");
    expect(useDiReviewStore.getState().reviewQueue).toBe(prev);
  });
});

// ── openForReview ─────────────────────────────────────────────────────────────

describe("openForReview", () => {
  it("clears the previous context immediately, then loads detail, events and similar", async () => {
    useDiReviewStore.setState({
      activeReviewDi: makeDi({ id: 99 }),
      reviewEvents: [makeEvent({ id: 99 })],
      similarDis: [makeSummary({ id: 99 })],
      error: "old",
    });
    const di = makeDi({ id: 3 });
    const similar = [makeSummary()];
    const events = [makeEvent({ di_id: 3 })];
    const d = deferred<DiGetResponse>();
    mockGetDi.mockReturnValueOnce(d.promise);
    mockGetDiReviewEvents.mockResolvedValueOnce(events);

    const p = useDiReviewStore.getState().openForReview(3);
    let s = useDiReviewStore.getState();
    expect(s.activeReviewDi).toBeNull();
    expect(s.reviewEvents).toEqual([]);
    expect(s.similarDis).toEqual([]);
    expect(s.error).toBeNull();
    expect(mockGetDi).toHaveBeenCalledWith(3);
    expect(mockGetDiReviewEvents).toHaveBeenCalledWith(3);

    d.resolve(makeDetail(di, similar));
    await p;
    s = useDiReviewStore.getState();
    expect(s.activeReviewDi).toEqual(di);
    expect(s.reviewEvents).toEqual(events);
    expect(s.similarDis).toEqual(similar);
  });

  it("swallows a failure into error and leaves the context empty", async () => {
    mockGetDi.mockRejectedValueOnce({ code: "NOT_FOUND", message: "DI 3 not found" });
    mockGetDiReviewEvents.mockResolvedValueOnce([]);

    await expect(useDiReviewStore.getState().openForReview(3)).resolves.toBeUndefined();
    expect(useDiReviewStore.getState().error).toBe("DI 3 not found");
    expect(useDiReviewStore.getState().activeReviewDi).toBeNull();
  });
});

// ── Dialog state ──────────────────────────────────────────────────────────────

describe("dialog state", () => {
  it("open/close approval, rejection and return dialogs independently", () => {
    const a = makeDi({ id: 1 });
    const r = makeDi({ id: 2 });
    const t = makeDi({ id: 3 });
    const store = useDiReviewStore.getState();

    store.openApproval(a);
    store.openRejection(r);
    store.openReturn(t);
    let s = useDiReviewStore.getState();
    expect(s.approvalDi).toEqual(a);
    expect(s.rejectionDi).toEqual(r);
    expect(s.returnDi_).toEqual(t);

    store.closeApproval();
    s = useDiReviewStore.getState();
    expect(s.approvalDi).toBeNull();
    expect(s.rejectionDi).toEqual(r);

    store.closeRejection();
    store.closeReturn();
    s = useDiReviewStore.getState();
    expect(s.rejectionDi).toBeNull();
    expect(s.returnDi_).toBeNull();
  });
});

// ── Triage actions sharing the same shape ─────────────────────────────────────

describe("screen", () => {
  it("screens, sets the active DI, reloads the queue and returns the result", async () => {
    const updated = makeDi({ status: "awaiting_approval", row_version: 2 });
    const d = deferred<InterventionRequest>();
    mockScreenDi.mockReturnValueOnce(d.promise);
    const input = {
      di_id: 1,
      actor_id: 7,
      expected_row_version: 1,
      validated_urgency: "high",
      classification_code_id: 4,
      reviewer_note: "ok",
    };

    const p = useDiReviewStore.getState().screen(input);
    expect(useDiReviewStore.getState().saving).toBe(true);
    d.resolve(updated);

    await expect(p).resolves.toEqual(updated);
    expect(mockScreenDi).toHaveBeenCalledWith(input);
    expect(mockListDis).toHaveBeenCalledWith(QUEUE_FILTER);
    expect(useDiReviewStore.getState().activeReviewDi).toEqual(updated);
    expect(useDiReviewStore.getState().saving).toBe(false);
  });

  it("sets error, clears saving and rethrows on failure", async () => {
    const err = new Error("stale");
    mockScreenDi.mockRejectedValueOnce(err);

    await expect(
      useDiReviewStore.getState().screen({
        di_id: 1,
        actor_id: 7,
        expected_row_version: 1,
        validated_urgency: "high",
      }),
    ).rejects.toBe(err);
    expect(useDiReviewStore.getState().error).toBe("stale");
    expect(useDiReviewStore.getState().saving).toBe(false);
    expect(mockListDis).not.toHaveBeenCalled();
  });

  it("still resolves when the queue reload fails afterwards (reload swallows its error)", async () => {
    const updated = makeDi({ row_version: 2 });
    mockScreenDi.mockResolvedValueOnce(updated);
    mockListDis.mockRejectedValueOnce(new Error("list failed"));

    await expect(
      useDiReviewStore.getState().screen({
        di_id: 1,
        actor_id: 7,
        expected_row_version: 1,
        validated_urgency: "high",
      }),
    ).resolves.toEqual(updated);
    expect(useDiReviewStore.getState().error).toBe("list failed");
  });
});

type VoidAction = {
  name: string;
  run: () => Promise<void>;
  service: ReturnType<typeof vi.fn>;
  expectedArg: unknown;
};

function voidActions(): VoidAction[] {
  const store = useDiReviewStore.getState();
  const returnInput = { di_id: 1, actor_id: 7, expected_row_version: 1, reviewer_note: "why?" };
  const closeInput = {
    di_id: 1,
    actor_id: 7,
    expected_row_version: 1,
    disposition_code: "duplicate",
    notes: "dup",
    related_di_id: 4,
  };
  const cancelInput = { di_id: 1, actor_id: 42, expected_row_version: 1, notes: "mine" };
  const deferInput = {
    di_id: 1,
    actor_id: 7,
    expected_row_version: 1,
    deferred_until: "2026-05-01",
    reason_code: "WAIT_PARTS",
    notes: null,
  };
  const reactivateInput = { di_id: 1, actor_id: 7, expected_row_version: 2, notes: null };
  return [
    {
      name: "returnForClarification",
      run: () => store.returnForClarification(returnInput),
      service: mockReturnDi,
      expectedArg: returnInput,
    },
    {
      name: "closeWithDisposition",
      run: () => store.closeWithDisposition(closeInput),
      service: mockCloseDi,
      expectedArg: closeInput,
    },
    {
      name: "cancelOwn",
      run: () => store.cancelOwn(cancelInput),
      service: mockCancelOwnDi,
      expectedArg: cancelInput,
    },
    {
      name: "defer",
      run: () => store.defer(deferInput),
      service: mockDeferDi,
      expectedArg: deferInput,
    },
    {
      name: "reactivate",
      run: () => store.reactivate(reactivateInput),
      service: mockReactivateDi,
      expectedArg: reactivateInput,
    },
    {
      name: "closeAsNonExecutable",
      run: () => store.closeAsNonExecutable(1, 3, "no work"),
      service: mockCloseNonExec,
      expectedArg: { di_id: 1, actor_id: 0, expected_row_version: 3, notes: "no work" },
    },
    {
      name: "archive",
      run: () => store.archive(1, 5, "done"),
      service: mockArchiveDi,
      expectedArg: { di_id: 1, actor_id: 0, expected_row_version: 5, notes: "done" },
    },
  ];
}

describe.each(voidActions().map((a) => [a.name] as const))("%s", (name) => {
  function action(): VoidAction {
    const found = voidActions().find((a) => a.name === name);
    if (!found) throw new Error(`unknown action ${name}`);
    return found;
  }

  it("calls the service with the exact input, sets the active DI and reloads the queue", async () => {
    const a = action();
    const updated = makeDi({ row_version: 9 });
    const d = deferred<InterventionRequest>();
    a.service.mockReturnValueOnce(d.promise);

    const p = a.run();
    expect(useDiReviewStore.getState().saving).toBe(true);
    expect(useDiReviewStore.getState().error).toBeNull();
    d.resolve(updated);
    await expect(p).resolves.toBeUndefined();

    expect(a.service).toHaveBeenCalledTimes(1);
    expect(a.service).toHaveBeenCalledWith(a.expectedArg);
    expect(mockListDis).toHaveBeenCalledWith(QUEUE_FILTER);
    expect(useDiReviewStore.getState().activeReviewDi).toEqual(updated);
    expect(useDiReviewStore.getState().saving).toBe(false);
  });

  it("sets error, clears saving, rethrows and leaves the active DI unchanged on failure", async () => {
    const a = action();
    const prev = makeDi({ id: 1, row_version: 1 });
    useDiReviewStore.setState({ activeReviewDi: prev });
    const err = { code: "VERSION_CONFLICT", message: `${name} refused` };
    a.service.mockRejectedValueOnce(err);

    await expect(a.run()).rejects.toBe(err);
    expect(useDiReviewStore.getState().error).toBe(`${name} refused`);
    expect(useDiReviewStore.getState().saving).toBe(false);
    expect(useDiReviewStore.getState().activeReviewDi).toBe(prev);
    expect(mockListDis).not.toHaveBeenCalled();
  });
});

describe("closeAsNonExecutable / archive notes default", () => {
  it("send notes: null when notes are omitted", async () => {
    mockCloseNonExec.mockResolvedValueOnce(makeDi());
    mockArchiveDi.mockResolvedValueOnce(makeDi());

    await useDiReviewStore.getState().closeAsNonExecutable(1, 2);
    await useDiReviewStore.getState().archive(1, 3);

    expect(mockCloseNonExec).toHaveBeenCalledWith({
      di_id: 1,
      actor_id: 0,
      expected_row_version: 2,
      notes: null,
    });
    expect(mockArchiveDi).toHaveBeenCalledWith({
      di_id: 1,
      actor_id: 0,
      expected_row_version: 3,
      notes: null,
    });
  });
});

// ── reject ────────────────────────────────────────────────────────────────────

describe("reject", () => {
  it("closes with rejected_invalid and 'reason: notes' trimmed", async () => {
    mockCloseDi.mockResolvedValueOnce(makeDi({ status: "closed" }));

    await useDiReviewStore.getState().reject({
      di_id: 1,
      actor_id: 7,
      expected_row_version: 2,
      reason_code: "  NOT_A_FAULT ",
      notes: "  operator error  ",
    });

    expect(mockCloseDi).toHaveBeenCalledWith({
      di_id: 1,
      actor_id: 7,
      expected_row_version: 2,
      disposition_code: "rejected_invalid",
      notes: "NOT_A_FAULT: operator error",
      related_di_id: null,
    });
    expect(useDiReviewStore.getState().activeReviewDi?.status).toBe("closed");
  });

  it("uses only the reason when notes are empty or missing", async () => {
    mockCloseDi.mockResolvedValue(makeDi());
    await useDiReviewStore
      .getState()
      .reject({ di_id: 1, actor_id: 7, expected_row_version: 2, reason_code: "DUP", notes: " " });
    await useDiReviewStore
      .getState()
      .reject({ di_id: 1, actor_id: 7, expected_row_version: 2, reason_code: "DUP" });

    expect(mockCloseDi.mock.calls.map(([i]) => i.notes)).toEqual(["DUP", "DUP"]);
  });

  it("sends notes: null when both reason and notes are blank", async () => {
    mockCloseDi.mockResolvedValueOnce(makeDi());
    await useDiReviewStore
      .getState()
      .reject({ di_id: 1, actor_id: 7, expected_row_version: 2, reason_code: "  ", notes: null });
    expect(mockCloseDi).toHaveBeenCalledWith(expect.objectContaining({ notes: null }));
  });

  it("propagates a close failure with error set", async () => {
    mockCloseDi.mockRejectedValueOnce(new Error("denied"));
    await expect(
      useDiReviewStore
        .getState()
        .reject({ di_id: 1, actor_id: 7, expected_row_version: 2, reason_code: "X" }),
    ).rejects.toThrow("denied");
    expect(useDiReviewStore.getState().error).toBe("denied");
  });
});

// ── approve ───────────────────────────────────────────────────────────────────

describe("approve", () => {
  const input = { di_id: 1, actor_id: 7, expected_row_version: 1, notes: "go" };

  it("approves, returns the result shape and reloads the queue", async () => {
    const updated = makeDi({ status: "approved", row_version: 4 });
    const d = deferred<InterventionRequest>();
    mockApproveDi.mockReturnValueOnce(d.promise);

    const p = useDiReviewStore.getState().approve(input);
    expect(useDiReviewStore.getState().saving).toBe(true);
    d.resolve(updated);

    await expect(p).resolves.toEqual({
      approved: true,
      converted: false,
      woId: null,
      woCode: null,
      conversionError: null,
      approvedRowVersion: 4,
    });
    expect(mockApproveDi).toHaveBeenCalledWith(input);
    expect(mockListDis).toHaveBeenCalledWith(QUEUE_FILTER);
    expect(useDiReviewStore.getState().activeReviewDi).toEqual(updated);
    expect(useDiReviewStore.getState().saving).toBe(false);
  });

  it("refreshes the open approval dialog DI when it is the same DI", async () => {
    useDiReviewStore.setState({ approvalDi: makeDi({ id: 1, row_version: 1 }) });
    const updated = makeDi({ id: 1, status: "approved", row_version: 2 });
    mockApproveDi.mockResolvedValueOnce(updated);

    await useDiReviewStore.getState().approve(input);
    expect(useDiReviewStore.getState().approvalDi).toEqual(updated);
  });

  it("leaves the approval dialog DI alone when it is a different DI", async () => {
    const other = makeDi({ id: 2 });
    useDiReviewStore.setState({ approvalDi: other });
    mockApproveDi.mockResolvedValueOnce(makeDi({ id: 1, status: "approved" }));

    await useDiReviewStore.getState().approve(input);
    expect(useDiReviewStore.getState().approvalDi).toBe(other);
  });

  it("keeps approvalDi null when no dialog is open", async () => {
    mockApproveDi.mockResolvedValueOnce(makeDi({ id: 1, status: "approved" }));
    await useDiReviewStore.getState().approve(input);
    expect(useDiReviewStore.getState().approvalDi).toBeNull();
  });

  it("sets error, clears saving and rethrows on failure", async () => {
    const err = new Error("step-up required");
    mockApproveDi.mockRejectedValueOnce(err);

    await expect(useDiReviewStore.getState().approve(input)).rejects.toBe(err);
    expect(useDiReviewStore.getState().error).toBe("step-up required");
    expect(useDiReviewStore.getState().saving).toBe(false);
    expect(mockListDis).not.toHaveBeenCalled();
  });
});

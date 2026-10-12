/**
 * Behaviour tests for the sync orchestrator store: worker status mirroring (event + initial
 * fetch), app-store reflection, network listeners, operator actions, conflict inbox and
 * diagnostics export. Mocked: the sync and licence services (IPC boundary) and the Tauri event
 * bus. The app store and i18n are real.
 */

import { type Event, type EventCallback, listen } from "@tauri-apps/api/event";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n/config";
import { requestLicenseRecheck } from "@/services/product-license-service";
import {
  getSyncStateSummary,
  getSyncWorkerStatus,
  listSyncConflicts,
  listSyncReplayRuns,
  pauseSync,
  replaySyncFailures,
  reportNetworkOnline,
  resolveSyncConflict,
  resumeSync,
  runSyncNow,
} from "@/services/sync-service";
import type * as SyncServiceNs from "@/services/sync-service";
import { useAppStore } from "@/store/app-store";
import type {
  ReplaySyncFailuresResult,
  SyncConflictRecord,
  SyncReplayRun,
  SyncStateSummary,
  SyncWorkerStatus,
} from "@shared/ipc-types";

import { syncBlockerText, useSyncOrchestratorStore } from "../sync-orchestrator-store";

type SyncServiceModule = typeof SyncServiceNs;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

vi.mock("@/services/product-license-service", () => ({
  requestLicenseRecheck: vi.fn(),
}));

vi.mock("@/services/sync-service", async (importOriginal) => {
  const actual = await importOriginal<SyncServiceModule>();
  return {
    SYNC_WORKER_STATUS_EVENT: actual.SYNC_WORKER_STATUS_EVENT,
    SyncWorkerStatusSchema: actual.SyncWorkerStatusSchema,
    getSyncStateSummary: vi.fn(),
    getSyncWorkerStatus: vi.fn(),
    listSyncConflicts: vi.fn(),
    listSyncReplayRuns: vi.fn(),
    pauseSync: vi.fn(),
    replaySyncFailures: vi.fn(),
    reportNetworkOnline: vi.fn(),
    runSyncNow: vi.fn(),
    resolveSyncConflict: vi.fn(),
    resumeSync: vi.fn(),
  };
});

const mockListen = vi.mocked(listen);
const mockRecheck = vi.mocked(requestLicenseRecheck);
const mockSummary = vi.mocked(getSyncStateSummary);
const mockWorkerStatus = vi.mocked(getSyncWorkerStatus);
const mockListConflicts = vi.mocked(listSyncConflicts);
const mockListReplayRuns = vi.mocked(listSyncReplayRuns);
const mockPause = vi.mocked(pauseSync);
const mockReplay = vi.mocked(replaySyncFailures);
const mockReportOnline = vi.mocked(reportNetworkOnline);
const mockRunSyncNow = vi.mocked(runSyncNow);
const mockResolve = vi.mocked(resolveSyncConflict);
const mockResume = vi.mocked(resumeSync);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeWorker(overrides: Partial<SyncWorkerStatus> = {}): SyncWorkerStatus {
  return {
    state: "idle",
    blocker: null,
    paused_until: null,
    last_success_at: "2026-10-12T08:00:00Z",
    last_attempt_at: "2026-10-12T08:00:00Z",
    next_attempt_at: "2026-10-12T08:05:00Z",
    pending_outbox_count: 0,
    dead_letter_outbox_count: 0,
    unresolved_conflict_count: 0,
    media_pending_count: 0,
    media_failed_count: 0,
    media_download_pending_count: 0,
    media_download_failed_count: 0,
    consecutive_failures: 0,
    last_error: null,
    ...overrides,
  };
}

function makeConflict(overrides: Partial<SyncConflictRecord> = {}): SyncConflictRecord {
  return {
    id: 11,
    conflict_key: "ck-11",
    source_scope: "inbox",
    source_batch_id: null,
    linked_outbox_id: 501,
    linked_inbox_id: null,
    entity_type: "work_order",
    entity_sync_id: "wo-sync-1",
    operation: "update",
    conflict_type: "version_mismatch",
    local_payload_json: null,
    inbound_payload_json: null,
    authority_side: "server",
    checkpoint_token: null,
    auto_resolution_policy: "manual",
    requires_operator_review: true,
    recommended_action: "accept_remote",
    status: "new",
    resolution_action: null,
    resolution_note: null,
    resolved_by_id: null,
    resolved_at: null,
    row_version: 1,
    local_row_version: 3,
    inbound_row_version: 4,
    created_at: "2026-10-12T07:00:00Z",
    updated_at: "2026-10-12T07:00:00Z",
    ...overrides,
  };
}

function makeSummary(overrides: Partial<SyncStateSummary> = {}): SyncStateSummary {
  return {
    protocol_version: "v2",
    checkpoint: null,
    pending_outbox_count: 2,
    rejected_outbox_count: 3,
    inbox_error_count: 0,
    in_flight_outbox_count: 0,
    dead_letter_outbox_count: 0,
    deferred_inbox_count: 4,
    inbox_conflict_count: 1,
    unresolved_conflict_count: 1,
    ...overrides,
  };
}

function makeReplayRun(overrides: Partial<SyncReplayRun> = {}): SyncReplayRun {
  return {
    id: 1,
    replay_key: "replay-1",
    mode: "single_item",
    status: "completed",
    reason: "test",
    requested_by_id: 1,
    scope_json: null,
    pre_replay_checkpoint: null,
    post_replay_checkpoint: null,
    result_json: null,
    created_at: "2026-10-12T07:00:00Z",
    started_at: null,
    finished_at: null,
    ...overrides,
  };
}

function makeReplayResult(): ReplaySyncFailuresResult {
  return {
    run: makeReplayRun(),
    requeued_outbox_count: 1,
    transitioned_conflict_count: 1,
    checkpoint_token_after: null,
    guard_applied: false,
  };
}

function statusEvent(payload: unknown): Event<unknown> {
  return { event: "sync-worker-status", id: 1, payload };
}

function blockerText(key: string): string {
  return String(i18n.t(`shell:sync.worker.blocker.${key}`));
}

// ── Harness ───────────────────────────────────────────────────────────────────

let statusHandler: EventCallback<unknown> | null = null;
const unlisten = vi.fn();

function installListen() {
  mockListen.mockImplementation((_event, handler) => {
    statusHandler = handler as EventCallback<unknown>;
    return Promise.resolve(unlisten);
  });
}

function emit(payload: unknown) {
  if (!statusHandler) throw new Error("no status listener registered");
  statusHandler(statusEvent(payload));
}

function primeInbox(
  conflicts: SyncConflictRecord[] = [],
  runs: SyncReplayRun[] = [],
  summary = makeSummary(),
) {
  mockListConflicts.mockResolvedValue(conflicts);
  mockListReplayRuns.mockResolvedValue(runs);
  mockSummary.mockResolvedValue(summary);
}

async function initAndSettle() {
  useSyncOrchestratorStore.getState().initialize();
  await vi.waitFor(() => expect(mockWorkerStatus).toHaveBeenCalled());
  await Promise.resolve();
  await Promise.resolve();
}

const initialSync = useSyncOrchestratorStore.getState();

beforeAll(async () => {
  initI18n();
  await i18n.changeLanguage("en");
});
const initialApp = useAppStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  statusHandler = null;
  useSyncOrchestratorStore.setState(initialSync, true);
  useAppStore.setState(initialApp, true);
  installListen();
  mockWorkerStatus.mockRejectedValue(new Error("no session"));
  mockRunSyncNow.mockResolvedValue(undefined);
  mockReportOnline.mockResolvedValue(undefined);
  primeInbox();
});

afterEach(() => {
  // Module-level listeners survive store resets: always tear them down.
  useSyncOrchestratorStore.getState().shutdown();
  vi.useRealTimers();
});

// ── syncBlockerText ───────────────────────────────────────────────────────────

describe("syncBlockerText", () => {
  it("returns null for no blocker", () => {
    expect(syncBlockerText(null)).toBeNull();
    expect(syncBlockerText("")).toBeNull();
  });

  it.each(["not_licensed", "not_activated", "activation_refused", "tenant_config_mismatch"])(
    "uses the dedicated text for %s",
    (code) => {
      expect(syncBlockerText(code)).toBe(blockerText(code));
    },
  );

  it("falls back to the generic text for unknown blockers", () => {
    expect(syncBlockerText("something_new")).toBe(blockerText("other"));
    expect(syncBlockerText("something_new")).toBe("Sync is stopped.");
    expect(syncBlockerText("not_licensed")).toBe("Sync is not included in this device's licence.");
  });
});

// ── initialize / shutdown ─────────────────────────────────────────────────────

describe("initialize", () => {
  it("subscribes to the worker status event and fetches the initial status", async () => {
    const worker = makeWorker({ pending_outbox_count: 5 });
    mockWorkerStatus.mockResolvedValueOnce(worker);

    useSyncOrchestratorStore.getState().initialize();
    expect(useSyncOrchestratorStore.getState().initialized).toBe(true);

    await vi.waitFor(() => expect(useSyncOrchestratorStore.getState().worker).toEqual(worker));
    expect(mockListen).toHaveBeenCalledWith("sync-worker-status", expect.any(Function));
    expect(useAppStore.getState().syncStatus.pendingCount).toBe(5);
  });

  it("is idempotent while initialized", async () => {
    await initAndSettle();
    useSyncOrchestratorStore.getState().initialize();
    await Promise.resolve();
    expect(mockListen).toHaveBeenCalledTimes(1);
    expect(mockWorkerStatus).toHaveBeenCalledTimes(1);
  });

  it("keeps an empty worker when the initial status fetch fails", async () => {
    await initAndSettle();
    expect(useSyncOrchestratorStore.getState().worker).toBeNull();
  });

  it("still fetches the status when the event bus is unavailable", async () => {
    mockListen.mockRejectedValueOnce(new Error("no tauri"));
    const worker = makeWorker();
    mockWorkerStatus.mockResolvedValueOnce(worker);

    useSyncOrchestratorStore.getState().initialize();
    await vi.waitFor(() => expect(useSyncOrchestratorStore.getState().worker).toEqual(worker));
  });

  it("applies valid status events and ignores malformed payloads", async () => {
    await initAndSettle();
    const worker = makeWorker({ state: "running", pending_outbox_count: 2 });

    emit({ state: "bogus" });
    expect(useSyncOrchestratorStore.getState().worker).toBeNull();

    emit(worker);
    expect(useSyncOrchestratorStore.getState().worker).toEqual(worker);
    expect(useAppStore.getState().syncStatus.state).toBe("running");
  });

  it("drops the listener if shutdown happened before subscription resolved", async () => {
    let resolveListen: (fn: () => void) => void = () => {};
    mockListen.mockImplementationOnce(
      () =>
        new Promise((res) => {
          resolveListen = res;
        }),
    );

    useSyncOrchestratorStore.getState().initialize();
    useSyncOrchestratorStore.getState().shutdown();
    resolveListen(unlisten);

    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    expect(useSyncOrchestratorStore.getState().initialized).toBe(false);
  });

  // BUG: React StrictMode (main.tsx) runs AppShell's effect as initialize → shutdown → initialize
  // synchronously. Both pending `listen` calls then resolve while `initialized` is true again, so
  // the second overwrites `unlistenStatus` and the first subscription is never released: the
  // status handler stays registered twice and the final shutdown only removes one of them.
  it.skip("BUG: initialize → shutdown → initialize leaks the first status listener", async () => {
    const unlistenA = vi.fn();
    const unlistenB = vi.fn();
    let resolveA: (fn: () => void) => void = () => {};
    let resolveB: (fn: () => void) => void = () => {};
    mockListen
      .mockImplementationOnce(
        () =>
          new Promise((res) => {
            resolveA = res;
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((res) => {
            resolveB = res;
          }),
      );

    const store = useSyncOrchestratorStore.getState();
    store.initialize();
    store.shutdown();
    useSyncOrchestratorStore.getState().initialize();
    resolveA(unlistenA);
    resolveB(unlistenB);
    await vi.waitFor(() => expect(mockWorkerStatus).toHaveBeenCalledTimes(2));

    useSyncOrchestratorStore.getState().shutdown();
    expect(unlistenA).toHaveBeenCalledTimes(1);
    expect(unlistenB).toHaveBeenCalledTimes(1);
  });

  it("wires online/offline window events to the app store and the worker", async () => {
    await initAndSettle();

    window.dispatchEvent(new Event("offline"));
    expect(useAppStore.getState().isOnline).toBe(false);

    window.dispatchEvent(new Event("online"));
    expect(useAppStore.getState().isOnline).toBe(true);
    expect(mockReportOnline).toHaveBeenCalledTimes(1);
  });

  it("ignores a failing online report", async () => {
    mockReportOnline.mockRejectedValueOnce(new Error("no session"));
    await initAndSettle();
    window.dispatchEvent(new Event("online"));
    await Promise.resolve();
    expect(useAppStore.getState().isOnline).toBe(true);
  });
});

describe("shutdown", () => {
  it("unsubscribes the status event, removes network listeners and resets initialized", async () => {
    await initAndSettle();
    useSyncOrchestratorStore.getState().shutdown();

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(useSyncOrchestratorStore.getState().initialized).toBe(false);

    useAppStore.setState({ isOnline: true });
    window.dispatchEvent(new Event("offline"));
    expect(useAppStore.getState().isOnline).toBe(true);
  });

  it("is safe to call when never initialized", () => {
    expect(() => useSyncOrchestratorStore.getState().shutdown()).not.toThrow();
    expect(unlisten).not.toHaveBeenCalled();
  });
});

// ── Worker status → app store ─────────────────────────────────────────────────

describe("worker status reflection in the app store", () => {
  it.each<[SyncWorkerStatus["state"], string]>([
    ["starting", "scheduled"],
    ["running", "running"],
    ["paused", "paused"],
    ["blocked", "blocked"],
    ["error", "error"],
    ["offline", "degraded"],
    ["retrying", "degraded"],
    ["idle", "idle"],
  ])("maps worker state %s to app sync state %s", async (state, expected) => {
    await initAndSettle();
    emit(makeWorker({ state }));
    expect(useAppStore.getState().syncStatus.state).toBe(expected);
  });

  it("reports idle as degraded when conflicts or dead letters remain", async () => {
    await initAndSettle();
    emit(makeWorker({ unresolved_conflict_count: 1 }));
    expect(useAppStore.getState().syncStatus.state).toBe("degraded");
    emit(makeWorker({ dead_letter_outbox_count: 2 }));
    expect(useAppStore.getState().syncStatus.state).toBe("degraded");
  });

  it("copies counts, last success, retry attempt and the blocker text", async () => {
    await initAndSettle();
    emit(
      makeWorker({
        state: "blocked",
        blocker: "not_activated",
        pending_outbox_count: 7,
        consecutive_failures: 3,
        last_success_at: "2026-10-11T00:00:00Z",
      }),
    );
    expect(useAppStore.getState().syncStatus).toEqual({
      state: "blocked",
      pendingCount: 7,
      lastSyncAt: "2026-10-11T00:00:00Z",
      errorMessage: null,
      blockerReason: blockerText("not_activated"),
      retryAttempt: 3,
    });
  });

  it.each<SyncWorkerStatus["state"]>(["error", "retrying"])(
    "shows the last error message only in %s",
    async (state) => {
      await initAndSettle();
      const last_error = { code: "rate_limited", message: "slow down", http_status: 429, at: "x" };
      emit(makeWorker({ state, last_error }));
      expect(useAppStore.getState().syncStatus.errorMessage).toBe("slow down");

      emit(makeWorker({ state: "running", last_error }));
      expect(useAppStore.getState().syncStatus.errorMessage).toBeNull();
    },
  );

  it("errorMessage is null in error state without a last_error", async () => {
    await initAndSettle();
    emit(makeWorker({ state: "error", last_error: null }));
    expect(useAppStore.getState().syncStatus.errorMessage).toBeNull();
  });

  it("marks the app offline for an offline worker and back online from navigator", async () => {
    await initAndSettle();
    emit(makeWorker({ state: "offline" }));
    expect(useAppStore.getState().isOnline).toBe(false);
    emit(makeWorker({ state: "idle" }));
    expect(useAppStore.getState().isOnline).toBe(navigator.onLine);
  });
});

// ── Worker status side effects ────────────────────────────────────────────────

describe("worker status side effects", () => {
  it("requests a licence recheck once when activation is refused", async () => {
    await initAndSettle();
    const refused = makeWorker({
      state: "blocked",
      blocker: "activation_refused",
      last_error: { code: "device_revoked", message: "revoked", http_status: 403, at: "x" },
    });

    emit(refused);
    emit(refused);

    expect(mockRecheck).toHaveBeenCalledTimes(1);
    expect(mockRecheck).toHaveBeenCalledWith("sync_device_revoked");
  });

  it("uses the blocker code as the recheck reason when there is no last error", async () => {
    await initAndSettle();
    emit(makeWorker({ state: "blocked", blocker: "activation_refused" }));
    expect(mockRecheck).toHaveBeenCalledWith("sync_activation_refused");
  });

  it("requests a new recheck after the blocker cleared and came back", async () => {
    await initAndSettle();
    emit(makeWorker({ blocker: "activation_refused" }));
    emit(makeWorker({ blocker: null }));
    emit(makeWorker({ blocker: "activation_refused" }));
    expect(mockRecheck).toHaveBeenCalledTimes(2);
  });

  it("refreshes the inbox when conflict or dead-letter counts change", async () => {
    await initAndSettle();
    const conflicts = [makeConflict()];
    primeInbox(conflicts);

    emit(makeWorker());
    expect(mockListConflicts).not.toHaveBeenCalled(); // first status: no previous

    emit(makeWorker());
    expect(mockListConflicts).not.toHaveBeenCalled(); // unchanged counts

    emit(makeWorker({ unresolved_conflict_count: 1 }));
    await vi.waitFor(() =>
      expect(useSyncOrchestratorStore.getState().conflictInbox).toEqual(conflicts),
    );

    emit(makeWorker({ unresolved_conflict_count: 1, dead_letter_outbox_count: 1 }));
    expect(mockListConflicts).toHaveBeenCalledTimes(2);
  });

  it("swallows a failing background inbox refresh", async () => {
    await initAndSettle();
    mockListConflicts.mockRejectedValue(new Error("boom"));
    emit(makeWorker());
    emit(makeWorker({ unresolved_conflict_count: 2 }));
    await vi.waitFor(() => expect(mockListConflicts).toHaveBeenCalled());
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();
  });
});

// ── Operator actions ──────────────────────────────────────────────────────────

describe("operator actions", () => {
  it("runSyncNow forwards the request and clears a previous action error", async () => {
    useSyncOrchestratorStore.setState({ actionError: "old" });
    await useSyncOrchestratorStore.getState().runSyncNow();
    expect(mockRunSyncNow).toHaveBeenCalledTimes(1);
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();
  });

  it("runSyncNow records a failure without throwing", async () => {
    mockRunSyncNow.mockRejectedValueOnce({ code: "X", message: "worker busy" });
    await expect(useSyncOrchestratorStore.getState().runSyncNow()).resolves.toBeUndefined();
    expect(useSyncOrchestratorStore.getState().actionError).toBe("worker busy");
  });

  it("pauseForMinutes passes the minutes", async () => {
    mockPause.mockResolvedValueOnce("2026-10-12T09:00:00Z");
    useSyncOrchestratorStore.setState({ actionError: "old" });
    await useSyncOrchestratorStore.getState().pauseForMinutes(30);
    expect(mockPause).toHaveBeenCalledWith(30);
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();
  });

  it("pauseForMinutes records a failure without throwing", async () => {
    mockPause.mockRejectedValueOnce(new Error("not allowed"));
    await expect(useSyncOrchestratorStore.getState().pauseForMinutes(5)).resolves.toBeUndefined();
    expect(useSyncOrchestratorStore.getState().actionError).toBe("not allowed");
  });

  it("resumeNow forwards and records failures", async () => {
    mockResume.mockResolvedValueOnce(undefined);
    await useSyncOrchestratorStore.getState().resumeNow();
    expect(mockResume).toHaveBeenCalledTimes(1);
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();

    mockResume.mockRejectedValueOnce("resume failed");
    await useSyncOrchestratorStore.getState().resumeNow();
    expect(useSyncOrchestratorStore.getState().actionError).toBe("resume failed");
  });
});

// ── runReplayRecovery ─────────────────────────────────────────────────────────

describe("runReplayRecovery", () => {
  it("targets the first inbox conflict, then refreshes and syncs", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-12T10:00:00Z"));
    useSyncOrchestratorStore.setState({
      conflictInbox: [makeConflict({ id: 11, linked_outbox_id: 501 }), makeConflict({ id: 12 })],
      actionError: "old",
    });
    mockReplay.mockResolvedValueOnce(makeReplayResult());
    const refreshed = [makeConflict({ id: 12 })];
    primeInbox(refreshed, [makeReplayRun(), makeReplayRun({ id: 2 })]);

    await useSyncOrchestratorStore.getState().runReplayRecovery();

    expect(mockReplay).toHaveBeenCalledWith({
      replay_key: `replay-${Date.parse("2026-10-12T10:00:00Z")}`,
      mode: "single_item",
      reason: "Operator-triggered replay recovery",
      conflict_id: 11,
      outbox_id: 501,
      server_batch_id: null,
      window_start: null,
      window_end: null,
      checkpoint_token: null,
    });
    const s = useSyncOrchestratorStore.getState();
    expect(s.conflictInbox).toEqual(refreshed);
    expect(s.replayHistoryCount).toBe(2);
    expect(s.actionError).toBeNull();
    expect(mockRunSyncNow).toHaveBeenCalledTimes(1);
    const replayOrder = mockReplay.mock.invocationCallOrder[0] ?? 0;
    const syncOrder = mockRunSyncNow.mock.invocationCallOrder[0] ?? 0;
    expect(replayOrder).toBeLessThan(syncOrder);
  });

  it("lets the caller override the scope", async () => {
    mockReplay.mockResolvedValueOnce(makeReplayResult());
    await useSyncOrchestratorStore.getState().runReplayRecovery({
      mode: "window",
      window_start: "2026-10-01",
      window_end: "2026-10-02",
      reason: "custom",
    });
    expect(mockReplay).toHaveBeenCalledWith(
      expect.objectContaining({
        mode: "window",
        window_start: "2026-10-01",
        window_end: "2026-10-02",
        reason: "custom",
        conflict_id: null,
        outbox_id: null,
      }),
    );
  });

  it("records a replay failure and does not refresh or sync", async () => {
    mockReplay.mockRejectedValueOnce(new Error("replay refused"));
    await expect(useSyncOrchestratorStore.getState().runReplayRecovery()).resolves.toBeUndefined();
    expect(useSyncOrchestratorStore.getState().actionError).toBe("replay refused");
    expect(mockListConflicts).not.toHaveBeenCalled();
    expect(mockRunSyncNow).not.toHaveBeenCalled();
  });

  it("records a refresh failure after a successful replay and skips the sync", async () => {
    mockReplay.mockResolvedValueOnce(makeReplayResult());
    mockSummary.mockRejectedValueOnce(new Error("summary failed"));
    await useSyncOrchestratorStore.getState().runReplayRecovery();
    expect(useSyncOrchestratorStore.getState().actionError).toBe("summary failed");
    expect(mockRunSyncNow).not.toHaveBeenCalled();
  });
});

// ── resolveConflictAction ─────────────────────────────────────────────────────

describe("resolveConflictAction", () => {
  it("replaces the conflict in the inbox, then refreshes from the server", async () => {
    const a = makeConflict({ id: 11 });
    const b = makeConflict({ id: 12 });
    useSyncOrchestratorStore.setState({ conflictInbox: [a, b] });
    const resolved = makeConflict({ id: 11, status: "resolved", resolution_action: "dismiss" });
    mockResolve.mockResolvedValueOnce(resolved);

    let inboxAfterResolve: SyncConflictRecord[] = [];
    mockListConflicts.mockImplementationOnce(() => {
      inboxAfterResolve = useSyncOrchestratorStore.getState().conflictInbox;
      return Promise.resolve([b]);
    });

    const input = {
      conflict_id: 11,
      expected_row_version: 1,
      action: "dismiss" as const,
      resolution_note: "noise",
    };
    await useSyncOrchestratorStore.getState().resolveConflictAction(input);

    expect(mockResolve).toHaveBeenCalledWith(input);
    expect(inboxAfterResolve).toEqual([resolved, b]);
    expect(useSyncOrchestratorStore.getState().conflictInbox).toEqual([b]);
    expect(mockRunSyncNow).not.toHaveBeenCalled();
  });

  it.each(["accept_local", "renumber_local"] as const)(
    "triggers an immediate sync after %s",
    async (action) => {
      mockResolve.mockResolvedValueOnce(makeConflict({ status: "resolved" }));
      await useSyncOrchestratorStore
        .getState()
        .resolveConflictAction({ conflict_id: 11, expected_row_version: 1, action });
      expect(mockRunSyncNow).toHaveBeenCalledTimes(1);
    },
  );

  it("ignores a failing follow-up sync", async () => {
    mockResolve.mockResolvedValueOnce(makeConflict({ status: "resolved" }));
    mockRunSyncNow.mockRejectedValueOnce(new Error("busy"));
    await expect(
      useSyncOrchestratorStore.getState().resolveConflictAction({
        conflict_id: 11,
        expected_row_version: 1,
        action: "accept_local",
      }),
    ).resolves.toBeUndefined();
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();
  });

  it("rethrows a resolve failure without touching the inbox or actionError", async () => {
    const inbox = [makeConflict()];
    useSyncOrchestratorStore.setState({ conflictInbox: inbox });
    const err = new Error("stale conflict");
    mockResolve.mockRejectedValueOnce(err);

    await expect(
      useSyncOrchestratorStore
        .getState()
        .resolveConflictAction({ conflict_id: 11, expected_row_version: 1, action: "dismiss" }),
    ).rejects.toBe(err);
    expect(useSyncOrchestratorStore.getState().conflictInbox).toBe(inbox);
    expect(useSyncOrchestratorStore.getState().actionError).toBeNull();
    expect(mockListConflicts).not.toHaveBeenCalled();
  });
});

// ── refreshInbox ──────────────────────────────────────────────────────────────

describe("refreshInbox", () => {
  it("queries open conflicts, replay runs and summary, then stores the counts", async () => {
    const conflicts = [makeConflict(), makeConflict({ id: 12 })];
    const summary = makeSummary({ rejected_outbox_count: 6, deferred_inbox_count: 9 });
    primeInbox(conflicts, [makeReplayRun()], summary);

    await useSyncOrchestratorStore.getState().refreshInbox();

    expect(mockListConflicts).toHaveBeenCalledWith({
      statuses: ["new", "triaged", "escalated"],
      requires_operator_review: true,
      limit: 50,
    });
    expect(mockListReplayRuns).toHaveBeenCalledWith(20);
    expect(mockSummary).toHaveBeenCalledTimes(1);
    const s = useSyncOrchestratorStore.getState();
    expect(s.conflictInbox).toEqual(conflicts);
    expect(s.replayHistoryCount).toBe(1);
    expect(s.rejectedOutboxCount).toBe(6);
    expect(s.deferredInboxCount).toBe(9);
    expect(s.lastSummary).toEqual(summary);
  });

  it("rethrows and leaves state untouched when any query fails", async () => {
    mockListReplayRuns.mockRejectedValueOnce(new Error("runs failed"));
    await expect(useSyncOrchestratorStore.getState().refreshInbox()).rejects.toThrow("runs failed");
    const s = useSyncOrchestratorStore.getState();
    expect(s.conflictInbox).toEqual([]);
    expect(s.lastSummary).toBeNull();
  });
});

// ── exportDiagnostics ─────────────────────────────────────────────────────────

describe("exportDiagnostics", () => {
  it("snapshots worker, summary, online state and a trimmed conflict list", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-12T10:00:00Z"));
    const worker = makeWorker({ state: "running" });
    const summary = makeSummary();
    useSyncOrchestratorStore.setState({
      worker,
      lastSummary: summary,
      conflictInbox: [makeConflict({ id: 11, status: "triaged" })],
    });

    expect(useSyncOrchestratorStore.getState().exportDiagnostics()).toEqual({
      generatedAt: "2026-10-12T10:00:00.000Z",
      online: navigator.onLine,
      worker,
      summary,
      unresolvedConflicts: [
        { id: 11, entity_type: "work_order", conflict_type: "version_mismatch", status: "triaged" },
      ],
    });
  });

  it("exports nulls and an empty list before anything loaded", () => {
    const out = useSyncOrchestratorStore.getState().exportDiagnostics();
    expect(out.worker).toBeNull();
    expect(out.summary).toBeNull();
    expect(out.unresolvedConflicts).toEqual([]);
  });
});

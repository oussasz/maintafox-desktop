/**
 * sync-service: IPC contract of the Rust sync worker (Phase 2.3) and the sync summary.
 *
 * Each wrapper must call the right Tauri command with the right arguments and reject responses
 * that do not match the schema, so a Rust/TS drift fails loudly instead of rendering wrong data.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { conflictActions } from "@/components/sync/sync-conflict-display";
import { mockInvoke } from "@/test/mocks/tauri";

import {
  getMediaFileStates,
  getSyncStateSummary,
  getSyncWorkerStatus,
  listOutboxItems,
  listSyncConflicts,
  pauseSync,
  reportNetworkOnline,
  resolveSyncConflict,
  resumeSync,
  retryMediaDownload,
  runSyncNow,
} from "../sync-service";

function workerStatus(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    state: "idle",
    blocker: null,
    paused_until: null,
    last_success_at: "2026-10-11T08:00:00Z",
    last_attempt_at: "2026-10-11T08:00:00Z",
    next_attempt_at: "2026-10-11T08:05:00Z",
    pending_outbox_count: 3,
    dead_letter_outbox_count: 0,
    unresolved_conflict_count: 1,
    media_pending_count: 0,
    media_failed_count: 0,
    media_download_pending_count: 2,
    media_download_failed_count: 0,
    consecutive_failures: 0,
    last_error: null,
    ...overrides,
  };
}

const EMPTY_SUMMARY = {
  protocol_version: "1.0.0",
  checkpoint: null,
  pending_outbox_count: 0,
  rejected_outbox_count: 0,
  inbox_error_count: 0,
  in_flight_outbox_count: 0,
  dead_letter_outbox_count: 0,
  deferred_inbox_count: 0,
  inbox_conflict_count: 0,
  unresolved_conflict_count: 0,
};

function conflictRecord(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 1,
    conflict_key: "inventory_count_lines:line-1:COUNT_SELF_APPROVAL",
    source_scope: "inbound",
    source_batch_id: "b1",
    linked_outbox_id: null,
    linked_inbox_id: 10,
    entity_type: "inventory_count_lines",
    entity_sync_id: "line-1",
    operation: "upsert",
    conflict_type: "COUNT_SELF_APPROVAL",
    local_payload_json: null,
    inbound_payload_json: "{}",
    authority_side: "server",
    checkpoint_token: null,
    auto_resolution_policy: "manual",
    requires_operator_review: true,
    recommended_action: "review",
    status: "new",
    resolution_action: null,
    resolution_note: null,
    resolved_by_id: null,
    resolved_at: null,
    row_version: 1,
    local_row_version: null,
    inbound_row_version: 3,
    created_at: "2026-10-24T10:05:00Z",
    updated_at: "2026-10-24T10:05:00Z",
    ...overrides,
  };
}

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("sync worker status", () => {
  it("reads the worker status from get_sync_worker_status", async () => {
    mockInvoke.mockResolvedValueOnce(workerStatus());
    const status = await getSyncWorkerStatus();
    expect(mockInvoke).toHaveBeenCalledWith("get_sync_worker_status", undefined);
    expect(status.state).toBe("idle");
    expect(status.pending_outbox_count).toBe(3);
    expect(status.media_download_pending_count).toBe(2);
  });

  it("keeps the last error details when the worker is retrying", async () => {
    mockInvoke.mockResolvedValueOnce(
      workerStatus({
        state: "retrying",
        consecutive_failures: 2,
        last_error: {
          code: "http_error",
          kind: "server_unavailable",
          message: "HTTP 503: unavailable",
          http_status: 503,
          at: "2026-10-11T08:01:00Z",
        },
      }),
    );
    const status = await getSyncWorkerStatus();
    expect(status.state).toBe("retrying");
    expect(status.last_error?.http_status).toBe(503);
    expect(status.last_error?.kind).toBe("server_unavailable");
  });

  it("rejects an unknown worker state instead of rendering it", async () => {
    mockInvoke.mockResolvedValueOnce(workerStatus({ state: "sleeping" }));
    await expect(getSyncWorkerStatus()).rejects.toThrow(
      /get_sync_worker_status response validation failed/,
    );
  });
});

describe("files received from other devices", () => {
  it("does not call Rust for an empty path list", async () => {
    await expect(getMediaFileStates([])).resolves.toEqual([]);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("returns the download state of each file", async () => {
    mockInvoke.mockResolvedValueOnce([
      { relativePath: "wo/1/a.jpg", state: "available", lastError: null, attemptCount: 1 },
      { relativePath: "wo/1/b.pdf", state: "failed", lastError: "hash mismatch", attemptCount: 3 },
    ]);
    const states = await getMediaFileStates(["wo/1/a.jpg", "wo/1/b.pdf"]);
    expect(mockInvoke).toHaveBeenCalledWith("get_media_file_states", {
      relativePaths: ["wo/1/a.jpg", "wo/1/b.pdf"],
    });
    expect(states.map((s) => s.state)).toEqual(["available", "failed"]);
    expect(states[1]?.lastError).toBe("hash mismatch");
  });

  it("rejects an unknown file state", async () => {
    mockInvoke.mockResolvedValueOnce([
      { relativePath: "x", state: "half", lastError: null, attemptCount: 0 },
    ]);
    await expect(getMediaFileStates(["x"])).rejects.toThrow(
      /get_media_file_states response validation failed/,
    );
  });

  it("queues a failed download again and returns how many were queued", async () => {
    mockInvoke.mockResolvedValueOnce(1);
    await expect(retryMediaDownload("wo/1/b.pdf")).resolves.toBe(1);
    expect(mockInvoke).toHaveBeenCalledWith("retry_media_download", {
      relativePath: "wo/1/b.pdf",
    });
  });
});

describe("worker controls", () => {
  it("run now, resume and network-online call their commands", async () => {
    mockInvoke.mockResolvedValue(null);
    await runSyncNow();
    await resumeSync();
    await reportNetworkOnline();
    expect(mockInvoke.mock.calls.map((c) => c[0])).toEqual([
      "run_sync_now",
      "resume_sync",
      "report_network_online",
    ]);
  });

  it("pause returns the end of the pause", async () => {
    mockInvoke.mockResolvedValueOnce("2026-10-11T09:00:00Z");
    await expect(pauseSync(60)).resolves.toBe("2026-10-11T09:00:00Z");
    expect(mockInvoke).toHaveBeenCalledWith("pause_sync", { minutes: 60 });
  });

  it("pause rejects a non-string answer", async () => {
    mockInvoke.mockResolvedValueOnce(42);
    await expect(pauseSync(60)).rejects.toThrow();
  });
});

describe("sync summary", () => {
  it("returns the summary from Rust", async () => {
    mockInvoke.mockResolvedValueOnce({ ...EMPTY_SUMMARY, pending_outbox_count: 7 });
    const summary = await getSyncStateSummary();
    expect(summary.pending_outbox_count).toBe(7);
  });

  it("degrades to an empty summary when the role lacks sync.view", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("Permission denied: sync.view"));
    await expect(getSyncStateSummary()).resolves.toEqual(EMPTY_SUMMARY);
  });

  it("propagates other errors", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("database is locked"));
    await expect(getSyncStateSummary()).rejects.toThrow("database is locked");
  });

  it("keeps the held count details of a conflict when listing and resolving", async () => {
    const hold = {
      session_code: "INV-2026-0042",
      counter: "Karim",
      posted_at: "2026-10-24T10:00:00Z",
    };
    mockInvoke.mockResolvedValueOnce([conflictRecord({ count_hold: hold })]);
    const listed = await listSyncConflicts();
    expect(listed[0]?.count_hold).toEqual(hold);

    mockInvoke.mockResolvedValueOnce(conflictRecord({ count_hold: hold, status: "resolved" }));
    const resolved = await resolveSyncConflict({
      conflict_id: 1,
      expected_row_version: 1,
      action: "retry_later",
      resolution_note: null,
    } as never);
    expect(resolved.count_hold).toEqual(hold);

    // A conflict of another kind carries none.
    mockInvoke.mockResolvedValueOnce([conflictRecord()]);
    const other = await listSyncConflicts();
    expect(other[0]?.count_hold).toBeNull();
  });

  it("keeps whether a refused count movement is held here, and the choices follow it", async () => {
    const movement = {
      entity_type: "inventory_transactions",
      entity_sync_id: "mov-1",
      conflict_type: "COUNT_APPROVAL_RECORD_INVALID",
    };
    const choices = async (
      overrides: Record<string, unknown>,
    ): Promise<{ held: boolean | null | undefined; actions: string[] | null }> => {
      mockInvoke.mockResolvedValueOnce([conflictRecord({ ...movement, ...overrides })]);
      const [c] = await listSyncConflicts();
      return {
        held: c?.history_row_held,
        actions: c
          ? conflictActions(c.conflict_type, false, c.entity_type, c.history_row_held)
          : null,
      };
    };
    // Held here: "keep mine" sends the movement again (the repair); "take theirs" is never offered.
    expect(await choices({ history_row_held: true })).toEqual({
      held: true,
      actions: ["accept_local", "escalate", "dismiss"],
    });
    // Not held here, or not said: only the two choices the server always accepts.
    expect(await choices({ history_row_held: false })).toEqual({
      held: false,
      actions: ["escalate", "dismiss"],
    });
    expect(await choices({})).toEqual({ held: null, actions: ["escalate", "dismiss"] });
  });

  it("lists outbox items with the given filter", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await expect(listOutboxItems({ status: "pending" } as never)).resolves.toEqual([]);
    expect(mockInvoke).toHaveBeenCalledWith("list_outbox_items", {
      filter: { status: "pending" },
    });
  });
});

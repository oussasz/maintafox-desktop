/**
 * updater-service.test.ts
 *
 * Tests for the updater service (IPC boundary) and the updater store wired to it.
 * The @tauri-apps/api/core invoke + Channel mocks are set up globally in src/test/setup.ts.
 *
 * Coverage goals (master plan Phase 5.5):
 *   1. Zod schemas accept the Rust shapes and reject bad ones.
 *   2. Commands are invoked with the locale and a progress channel (camelCase `onProgress`).
 *   3. Progress messages are validated before they reach the caller.
 *   4. Store state transitions with the real service.
 */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUpdaterStore } from "@/stores/updater-store";
import { fixtures, mockInvoke } from "@/test/mocks/tauri";
import type { UpdateProgressEvent } from "@shared/ipc-types";

import {
  InstallOutcomeSchema,
  UpdateCheckResultSchema,
  UpdateJournalViewSchema,
  UpdateProgressEventSchema,
  acknowledgeUpdateNotice,
  checkForUpdate,
  getUpdateJournal,
  installPendingUpdate,
  reinstallPreviousVersion,
} from "../updater-service";

const initialStoreState = useUpdaterStore.getState();

/** Channel passed as `onProgress` in the last invoke call of `command`. */
function progressChannelOf(command: string): { onmessage: (raw: unknown) => void } {
  const call = mockInvoke.mock.calls.find(([cmd]) => cmd === command);
  const args = call?.[1] as { onProgress?: { onmessage: (raw: unknown) => void } } | undefined;
  if (!args?.onProgress) throw new Error(`no progress channel passed to ${command}`);
  return args.onProgress;
}

beforeEach(() => {
  mockInvoke.mockReset();
  useUpdaterStore.setState(initialStoreState, true);
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
});

// ─────────────────────────────────────────────────────────────────────────────
// Schemas
// ─────────────────────────────────────────────────────────────────────────────

describe("updater schemas", () => {
  it("accept every check status the Rust layer sends", () => {
    for (const status of [
      "available",
      "up_to_date",
      "not_configured",
      "not_activated",
      "unreachable",
    ] as const) {
      expect(
        UpdateCheckResultSchema.safeParse({ ...fixtures.updateCheckNoUpdate, status }).success,
      ).toBe(true);
    }
    expect(UpdateCheckResultSchema.safeParse(fixtures.updateCheckAvailable).success).toBe(true);
  });

  it("reject the old 4-field check result and unknown statuses / kinds", () => {
    expect(
      UpdateCheckResultSchema.safeParse({
        available: false,
        version: null,
        notes: null,
        pub_date: null,
      }).success,
    ).toBe(false);
    expect(
      UpdateCheckResultSchema.safeParse({ ...fixtures.updateCheckNoUpdate, status: "maybe" })
        .success,
    ).toBe(false);
    expect(
      UpdateCheckResultSchema.safeParse({ ...fixtures.updateCheckAvailable, kind: "downgrade" })
        .success,
    ).toBe(false);
  });

  it("validate progress events", () => {
    expect(UpdateProgressEventSchema.safeParse({ event: "backing_up" }).success).toBe(true);
    expect(UpdateProgressEventSchema.safeParse({ event: "restoring_backup" }).success).toBe(true);
    expect(UpdateProgressEventSchema.safeParse({ event: "installing" }).success).toBe(true);
    expect(
      UpdateProgressEventSchema.safeParse({
        event: "downloading",
        downloaded_bytes: 10,
        total_bytes: null,
      }).success,
    ).toBe(true);
    expect(
      UpdateProgressEventSchema.safeParse({
        event: "downloading",
        downloaded_bytes: -1,
        total_bytes: 5,
      }).success,
    ).toBe(false);
    expect(UpdateProgressEventSchema.safeParse({ event: "exploding" }).success).toBe(false);
  });

  it("validate install outcomes and refusals", () => {
    expect(InstallOutcomeSchema.safeParse({ outcome: "restarting" }).success).toBe(true);
    for (const refusal of [
      { code: "rollback_backup_missing" },
      { code: "rollback_unsynced_changes", count: 3 },
      { code: "update_withdrawn" },
    ]) {
      expect(InstallOutcomeSchema.safeParse({ outcome: "refused", refusal }).success).toBe(true);
    }
    expect(
      InstallOutcomeSchema.safeParse({
        outcome: "refused",
        refusal: { code: "rollback_unsynced_changes" },
      }).success,
    ).toBe(false);
    expect(InstallOutcomeSchema.safeParse({ outcome: "done" }).success).toBe(false);
  });

  it("validate the update journal", () => {
    const journal = {
      state: "rolled_back",
      kind: "rollback",
      from_version: "1.2.0",
      to_version: "1.1.0",
      failure: null,
      notice_acknowledged: false,
    };
    expect(UpdateJournalViewSchema.safeParse(journal).success).toBe(true);
    expect(UpdateJournalViewSchema.safeParse({ ...journal, state: "unknown" }).success).toBe(false);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Service functions
// ─────────────────────────────────────────────────────────────────────────────

describe("updater service", () => {
  it("checkForUpdate passes the locale and returns the validated result", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckAvailable);

    const result = await checkForUpdate("fr");

    expect(mockInvoke).toHaveBeenCalledWith("check_for_update", { locale: "fr" });
    expect(result.version).toBe("1.2.0");
    expect(result.notes_fr).toContain("Corrections");
  });

  it("checkForUpdate rejects a malformed response", async () => {
    mockInvoke.mockResolvedValueOnce({ available: 1, version: null, notes: null, pub_date: null });

    await expect(checkForUpdate("en")).rejects.toThrow();
  });

  it("installPendingUpdate sends a progress channel and forwards valid events only", async () => {
    mockInvoke.mockResolvedValueOnce({ outcome: "restarting" });
    const events: UpdateProgressEvent[] = [];

    const outcome = await installPendingUpdate("en", (e) => events.push(e));

    expect(outcome).toEqual({ outcome: "restarting" });
    expect(mockInvoke).toHaveBeenCalledWith(
      "install_pending_update",
      expect.objectContaining({ locale: "en", onProgress: expect.anything() }),
    );
    const channel = progressChannelOf("install_pending_update");
    channel.onmessage({ event: "backing_up" });
    channel.onmessage({ event: "downloading", downloaded_bytes: 512, total_bytes: 1024 });
    channel.onmessage({ event: "not_a_phase" });
    expect(events).toEqual([
      { event: "backing_up" },
      { event: "downloading", downloaded_bytes: 512, total_bytes: 1024 },
    ]);
  });

  it("reinstallPreviousVersion returns refusals", async () => {
    mockInvoke.mockResolvedValueOnce({ outcome: "refused", refusal: { code: "update_withdrawn" } });

    const outcome = await reinstallPreviousVersion("fr", () => undefined);

    expect(outcome).toEqual({ outcome: "refused", refusal: { code: "update_withdrawn" } });
    expect(mockInvoke).toHaveBeenCalledWith(
      "reinstall_previous_version",
      expect.objectContaining({ locale: "fr" }),
    );
  });

  it("getUpdateJournal accepts null (never updated in the app)", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await expect(getUpdateJournal()).resolves.toBeNull();
  });

  it("acknowledgeUpdateNotice invokes the command", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await acknowledgeUpdateNotice();
    expect(mockInvoke).toHaveBeenCalledWith("acknowledge_update_notice", undefined);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Store wired to the real service
// ─────────────────────────────────────────────────────────────────────────────

describe("useUpdaterStore with the updater service", () => {
  it("records the check result", async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === "check_for_update") return fixtures.updateCheckAvailable;
      if (command === "get_product_license_onboarding_state") {
        return { complete: true, status: "active", pending_online_validation: false };
      }
      return null;
    });

    const { result } = renderHook(() => useUpdaterStore());
    await act(async () => {
      await result.current.check("en");
    });

    expect(result.current.isChecking).toBe(false);
    expect(result.current.lastCheckResult?.version).toBe("1.2.0");
    expect(result.current.checkError).toBeNull();
  });

  it("follows install progress sent on the channel", async () => {
    let finish: (value: unknown) => void = () => undefined;
    mockInvoke.mockImplementation(
      (command) =>
        new Promise((resolve) => {
          if (command === "install_pending_update") finish = resolve;
          else resolve(null);
        }),
    );

    const pending = useUpdaterStore.getState().install("en");
    const channel = progressChannelOf("install_pending_update");

    act(() => channel.onmessage({ event: "downloading", downloaded_bytes: 300, total_bytes: 600 }));
    expect(useUpdaterStore.getState().phase).toBe("downloading");
    expect(useUpdaterStore.getState().downloadedBytes).toBe(300);

    act(() => channel.onmessage({ event: "installing" }));
    expect(useUpdaterStore.getState().phase).toBe("installing");

    finish({ outcome: "restarting" });
    await act(async () => {
      await pending;
    });
    expect(useUpdaterStore.getState().phase).toBe("restarting");
  });
});

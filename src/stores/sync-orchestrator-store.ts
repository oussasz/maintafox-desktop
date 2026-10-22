/**
 * Sync state for the UI. Sync itself runs in Rust (background task `sync`, protocol v2) and keeps
 * running while the window is hidden; this store mirrors its status (`sync-worker-status` event),
 * forwards operator actions (sync now, pause, resume) and lists conflicts for the Sync Center.
 */
import { type UnlistenFn, listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { i18n } from "@/i18n/config";
import { createLatestRequestGuard } from "@/lib/latest-request";
import { requestLicenseRecheck } from "@/services/product-license-service";
import {
  SYNC_WORKER_STATUS_EVENT,
  SyncWorkerStatusSchema,
  getSyncStateSummary,
  getSyncWorkerStatus,
  listSyncConflicts,
  listSyncReplayRuns,
  pauseSync,
  replaySyncFailures,
  reportNetworkOnline,
  runSyncNow as requestSyncNow,
  resolveSyncConflict,
  resumeSync,
} from "@/services/sync-service";
import { type SyncStatus, useAppStore } from "@/store/app-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  ReplaySyncFailuresInput,
  ResolveSyncConflictInput,
  SyncConflictRecord,
  SyncStateSummary,
  SyncWorkerStatus,
} from "@shared/ipc-types";

/** Blocker codes with a dedicated text in `shell:sync.worker.blocker`. */
const KNOWN_BLOCKERS = new Set([
  "not_licensed",
  "not_activated",
  "activation_refused",
  "tenant_config_mismatch",
  "not_configured",
]);

export function syncBlockerText(blocker: string | null): string | null {
  if (!blocker) return null;
  const key = KNOWN_BLOCKERS.has(blocker) ? blocker : "other";
  return String(i18n.t(`shell:sync.worker.blocker.${key}`));
}

/** Top bar / status bar state for a worker status. */
function appSyncState(worker: SyncWorkerStatus): SyncStatus["state"] {
  switch (worker.state) {
    case "starting":
      return "scheduled";
    case "running":
      return "running";
    case "paused":
      return "paused";
    case "blocked":
      return "blocked";
    case "error":
      return "error";
    case "offline":
    case "retrying":
      return "degraded";
    case "idle":
      return worker.unresolved_conflict_count > 0 || worker.dead_letter_outbox_count > 0
        ? "degraded"
        : "idle";
  }
}

function reflectInAppStore(worker: SyncWorkerStatus) {
  const app = useAppStore.getState();
  app.setSyncStatus({
    state: appSyncState(worker),
    pendingCount: worker.pending_outbox_count,
    lastSyncAt: worker.last_success_at,
    errorMessage:
      worker.state === "error" || worker.state === "retrying"
        ? (worker.last_error?.message ?? null)
        : null,
    blockerReason: syncBlockerText(worker.blocker),
    retryAttempt: worker.consecutive_failures,
  });
  if (worker.state === "offline") {
    app.setOnline(false);
  } else if (typeof navigator !== "undefined") {
    app.setOnline(navigator.onLine);
  }
}

export interface SyncDiagnosticsExport {
  generatedAt: string;
  online: boolean;
  worker: SyncWorkerStatus | null;
  summary: SyncStateSummary | null;
  unresolvedConflicts: Array<
    Pick<SyncConflictRecord, "id" | "entity_type" | "conflict_type" | "status">
  >;
}

interface SyncOrchestratorState {
  initialized: boolean;
  /** Last status pushed by the Rust worker (`null` until the first one arrives). */
  worker: SyncWorkerStatus | null;
  /** Last failure of an operator action (sync now, pause, resume) for the Sync Center. */
  actionError: string | null;
  conflictInbox: SyncConflictRecord[];
  replayHistoryCount: number;
  rejectedOutboxCount: number;
  /** Received changes waiting for related data or a newer app version (retried automatically). */
  deferredInboxCount: number;
  lastSummary: SyncStateSummary | null;
  /**
   * The signed-in user holds `sync.view` (set by the top bar from the permission context; the store
   * has none). Until it is true the inbox is not refreshed on worker status changes: the refresh
   * runs without the user asking, and the server would refuse it.
   */
  canViewSync: boolean;

  initialize: () => void;
  shutdown: () => void;
  runSyncNow: () => Promise<void>;
  pauseForMinutes: (minutes: number) => Promise<void>;
  resumeNow: () => Promise<void>;
  runReplayRecovery: (scope?: Partial<ReplaySyncFailuresInput>) => Promise<void>;
  resolveConflictAction: (input: ResolveSyncConflictInput) => Promise<void>;
  refreshInbox: () => Promise<void>;
  exportDiagnostics: () => SyncDiagnosticsExport;
  setCanViewSync: (allowed: boolean) => void;
}

let unlistenStatus: UnlistenFn | null = null;
let removeNetworkListeners: (() => void) | null = null;
/** One "session" per initialize; shutdown makes the running one stale. */
const sessions = createLatestRequestGuard();

function applyWorkerStatus(next: SyncWorkerStatus) {
  const previous = useSyncOrchestratorStore.getState().worker;
  useSyncOrchestratorStore.setState({ worker: next });
  reflectInAppStore(next);
  // The control API refused this device: let the licence gate check the activation now (it may
  // re-claim with the cached key or record the revocation) instead of waiting for its schedule.
  if (next.blocker === "activation_refused" && previous?.blocker !== "activation_refused") {
    requestLicenseRecheck(`sync_${next.last_error?.code ?? "activation_refused"}`);
  }
  // Conflicts or dead letters changed: refresh the Sync Center list (only for who may read it).
  if (
    useSyncOrchestratorStore.getState().canViewSync &&
    previous &&
    (previous.unresolved_conflict_count !== next.unresolved_conflict_count ||
      previous.dead_letter_outbox_count !== next.dead_letter_outbox_count)
  ) {
    void useSyncOrchestratorStore
      .getState()
      .refreshInbox()
      .catch(() => {});
  }
}

export const useSyncOrchestratorStore = create<SyncOrchestratorState>()((set, get) => ({
  initialized: false,
  worker: null,
  actionError: null,
  conflictInbox: [],
  replayHistoryCount: 0,
  rejectedOutboxCount: 0,
  deferredInboxCount: 0,
  lastSummary: null,
  canViewSync: false,

  initialize: () => {
    if (get().initialized) return;
    // StrictMode runs initialize → shutdown → initialize back to back: a subscription that
    // resolves after its own session ended is released at once instead of being kept.
    const isCurrent = sessions.begin();
    set({ initialized: true });
    void (async () => {
      try {
        const unlisten = await listen<unknown>(SYNC_WORKER_STATUS_EVENT, (event) => {
          const parsed = SyncWorkerStatusSchema.safeParse(event.payload);
          if (parsed.success) applyWorkerStatus(parsed.data as SyncWorkerStatus);
        });
        if (isCurrent()) {
          unlistenStatus = unlisten;
        } else {
          unlisten();
        }
      } catch {
        // Outside Tauri (tests) there is no event bus; the store stays empty.
      }
      try {
        const status = await getSyncWorkerStatus();
        if (isCurrent()) applyWorkerStatus(status);
      } catch {
        // No session yet: the next status event fills the store.
      }
    })();
    if (typeof window !== "undefined" && !removeNetworkListeners) {
      const onOnline = () => {
        useAppStore.getState().setOnline(true);
        void reportNetworkOnline().catch(() => {});
      };
      const onOffline = () => useAppStore.getState().setOnline(false);
      window.addEventListener("online", onOnline);
      window.addEventListener("offline", onOffline);
      removeNetworkListeners = () => {
        window.removeEventListener("online", onOnline);
        window.removeEventListener("offline", onOffline);
      };
    }
  },

  shutdown: () => {
    sessions.invalidate();
    unlistenStatus?.();
    unlistenStatus = null;
    removeNetworkListeners?.();
    removeNetworkListeners = null;
    set({ initialized: false });
  },

  runSyncNow: async () => {
    set({ actionError: null });
    try {
      await requestSyncNow();
    } catch (error) {
      set({ actionError: toErrorMessage(error) });
    }
  },

  pauseForMinutes: async (minutes) => {
    set({ actionError: null });
    try {
      await pauseSync(minutes);
    } catch (error) {
      set({ actionError: toErrorMessage(error) });
    }
  },

  resumeNow: async () => {
    set({ actionError: null });
    try {
      await resumeSync();
    } catch (error) {
      set({ actionError: toErrorMessage(error) });
    }
  },

  runReplayRecovery: async (scope = {}) => {
    const first = get().conflictInbox[0];
    const payload: ReplaySyncFailuresInput = {
      replay_key: `replay-${Date.now()}`,
      mode: "single_item",
      reason: "Operator-triggered replay recovery",
      conflict_id: first?.id ?? null,
      outbox_id: first?.linked_outbox_id ?? null,
      server_batch_id: null,
      window_start: null,
      window_end: null,
      checkpoint_token: null,
      ...scope,
    };
    set({ actionError: null });
    try {
      await replaySyncFailures(payload);
      await get().refreshInbox();
      await requestSyncNow();
    } catch (error) {
      set({ actionError: toErrorMessage(error) });
    }
  },

  resolveConflictAction: async (input) => {
    const resolved = await resolveSyncConflict(input);
    set((state) => ({
      conflictInbox: state.conflictInbox.map((item) => (item.id === resolved.id ? resolved : item)),
    }));
    await get().refreshInbox();
    // "Keep mine" re-stages the local version: send it without waiting for the next cycle.
    // Renumbering also stages the local record's new number.
    if (input.action === "accept_local" || input.action === "renumber_local") {
      await requestSyncNow().catch(() => {});
    }
  },

  refreshInbox: async () => {
    const [conflicts, replayRuns, summary] = await Promise.all([
      listSyncConflicts({
        statuses: ["new", "triaged", "escalated"],
        requires_operator_review: true,
        limit: 50,
      }),
      listSyncReplayRuns(20),
      getSyncStateSummary(),
    ]);
    set({
      conflictInbox: conflicts,
      replayHistoryCount: replayRuns.length,
      rejectedOutboxCount: summary.rejected_outbox_count,
      deferredInboxCount: summary.deferred_inbox_count,
      lastSummary: summary,
    });
  },

  setCanViewSync: (allowed) => {
    if (get().canViewSync === allowed) return;
    // Losing the right (another user signed in) drops what the previous one could read.
    set(
      allowed
        ? { canViewSync: true }
        : {
            canViewSync: false,
            conflictInbox: [],
            replayHistoryCount: 0,
            rejectedOutboxCount: 0,
            deferredInboxCount: 0,
            lastSummary: null,
          },
    );
  },

  exportDiagnostics: () => {
    const state = get();
    return {
      generatedAt: new Date().toISOString(),
      online: typeof navigator === "undefined" ? true : navigator.onLine,
      worker: state.worker,
      summary: state.lastSummary,
      unresolvedConflicts: state.conflictInbox.map((c) => ({
        id: c.id,
        entity_type: c.entity_type,
        conflict_type: c.conflict_type,
        status: c.status,
      })),
    };
  },
}));

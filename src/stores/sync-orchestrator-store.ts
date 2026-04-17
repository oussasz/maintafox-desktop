import { create } from "zustand";
import { persist } from "zustand/middleware";

import {
  getSyncStateSummary,
  listSyncConflicts,
  listSyncReplayRuns,
  replaySyncFailures,
  resolveSyncConflict,
} from "@/services/sync-service";
import {
  computeRetryDelayMs,
  defaultRetryPolicy,
  defaultSyncPolicyControls,
  evaluateSchedulingGate,
  normalizeRuntimeStateAfterRestart,
  shouldRetry,
  type RetryPolicy,
  type SyncPolicyControls,
  type SyncRunMode,
  type SyncRuntimeState,
} from "@/services/sync-orchestrator-core";
import { useAppStore } from "@/store/app-store";
import { toErrorMessage } from "@/utils/errors";
import type { ReplaySyncFailuresInput, ResolveSyncConflictInput, SyncConflictRecord } from "@shared/ipc-types";

type TimelineSeverity = "info" | "warning" | "error";

export interface SyncTimelineEvent {
  id: string;
  at: string;
  severity: TimelineSeverity;
  mode: SyncRunMode;
  event: string;
  message: string;
  correlationId: string;
  details?: Record<string, unknown>;
}

interface SyncTelemetryCounters {
  runsTotal: number;
  runsSucceeded: number;
  runsFailed: number;
  runsBlocked: number;
  retriesScheduled: number;
  totalDurationMs: number;
}

interface RetryState {
  attempt: number;
  nextRetryAt: string | null;
  lastError: string | null;
}

export interface SyncDiagnosticsExport {
  generatedAt: string;
  machineContext: {
    userAgent: string;
    language: string;
    platform: string;
    online: boolean;
  };
  runtime: {
    state: SyncRuntimeState;
    mode: SyncRunMode;
    blockerReason: string | null;
    checkpointToken: string | null;
    pendingBacklog: number;
    unresolvedConflicts: number;
    lastSuccessAt: string | null;
    nextScheduledAt: string | null;
  };
  policy: SyncPolicyControls;
  retry: RetryState;
  telemetry: SyncTelemetryCounters & {
    averageDurationMs: number;
  };
  timeline: SyncTimelineEvent[];
}

interface SyncOrchestratorState {
  initialized: boolean;
  runtimeState: SyncRuntimeState;
  activeMode: SyncRunMode;
  blockerReason: string | null;
  lastSuccessAt: string | null;
  lastRunStartedAt: string | null;
  lastRunFinishedAt: string | null;
  nextScheduledAt: string | null;
  checkpointToken: string | null;
  pendingBacklog: number;
  unresolvedConflicts: number;
  lastError: string | null;
  offlineSince: string | null;
  policy: SyncPolicyControls;
  retryPolicy: RetryPolicy;
  retry: RetryState;
  timeline: SyncTimelineEvent[];
  telemetry: SyncTelemetryCounters;
  conflictInbox: SyncConflictRecord[];
  replayHistoryCount: number;

  initialize: () => void;
  shutdown: () => void;
  scheduleNext: (delayMs?: number) => void;
  runSyncNow: (mode?: SyncRunMode) => Promise<void>;
  runReplayRecovery: (scope?: Partial<ReplaySyncFailuresInput>) => Promise<void>;
  resolveConflictAction: (input: ResolveSyncConflictInput) => Promise<void>;
  refreshInbox: () => Promise<void>;
  updatePolicy: (patch: Partial<SyncPolicyControls>) => void;
  pauseForMinutes: (minutes: number) => void;
  resumeNow: () => void;
  exportDiagnostics: () => SyncDiagnosticsExport;
}

let schedulerTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
let onlineHandlerBound = false;

function nowIso(): string {
  return new Date().toISOString();
}

function newCorrelationId(): string {
  return `${Date.now()}-${Math.random().toString(16).slice(2, 10)}`;
}

function pushTimeline(
  timeline: SyncTimelineEvent[],
  entry: Omit<SyncTimelineEvent, "id" | "at">,
): SyncTimelineEvent[] {
  const item: SyncTimelineEvent = {
    id: `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`,
    at: nowIso(),
    ...entry,
  };
  const next = [...timeline, item];
  return next.slice(Math.max(0, next.length - 250));
}

function redactSecrets(value: string): string {
  return value
    .replace(/(token|secret|password)\s*[:=]\s*["']?[^"',\s}]+["']?/gi, "$1=[REDACTED]")
    .replace(/[A-Za-z0-9_\-]{24,}\.[A-Za-z0-9_\-]{12,}\.[A-Za-z0-9_\-]{12,}/g, "[REDACTED_JWT]");
}

function clearSchedulerTimer() {
  if (schedulerTimer) {
    clearTimeout(schedulerTimer);
    schedulerTimer = null;
  }
}

function clearHeartbeatTimer() {
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
}

function reflectAppSyncState(
  runtimeState: SyncRuntimeState,
  pendingBacklog: number,
  lastSuccessAt: string | null,
  lastError: string | null,
  blockerReason: string | null,
  retryAttempt: number,
) {
  const appState = useAppStore.getState();
  appState.setSyncStatus({
    state: runtimeState,
    pendingCount: pendingBacklog,
    lastSyncAt: lastSuccessAt,
    errorMessage: lastError,
    blockerReason,
    retryAttempt,
  });
}

async function executeSyncRun(mode: SyncRunMode) {
  const startedAtMs = Date.now();
  const correlationId = newCorrelationId();
  const orchestrator = useSyncOrchestratorStore.getState();
  const gate = evaluateSchedulingGate(orchestrator.policy, {
    nowIso: nowIso(),
    isOnline: typeof navigator === "undefined" ? true : navigator.onLine,
    offlineSinceIso: orchestrator.offlineSince,
  });
  if (!gate.allowed) {
    useSyncOrchestratorStore.setState((state) => {
      const timeline = pushTimeline(state.timeline, {
        correlationId,
        severity: gate.state === "degraded" ? "warning" : "info",
        mode,
        event: "run_blocked",
        message: gate.blockerReason ?? "Sync blocked by policy gate.",
      });
      reflectAppSyncState(
        gate.state,
        state.pendingBacklog,
        state.lastSuccessAt,
        state.lastError,
        gate.blockerReason,
        state.retry.attempt,
      );
      return {
        runtimeState: gate.state,
        blockerReason: gate.blockerReason,
        activeMode: mode,
        timeline,
        telemetry: {
          ...state.telemetry,
          runsBlocked: state.telemetry.runsBlocked + 1,
        },
      };
    });
    return;
  }

  useSyncOrchestratorStore.setState((state) => {
    const timeline = pushTimeline(state.timeline, {
      correlationId,
      severity: "info",
      mode,
      event: "run_started",
      message: `Sync cycle started (${mode}).`,
    });
    reflectAppSyncState("running", state.pendingBacklog, state.lastSuccessAt, null, null, state.retry.attempt);
    return {
      runtimeState: "running",
      activeMode: mode,
      blockerReason: null,
      lastRunStartedAt: nowIso(),
      lastError: null,
      timeline,
    };
  });

  try {
    const summary = await getSyncStateSummary();
    const conflicts = await listSyncConflicts({
      statuses: ["new", "triaged", "escalated"],
      requires_operator_review: true,
      limit: 50,
    });
    const replayRuns = await listSyncReplayRuns(20);

    useSyncOrchestratorStore.setState((state) => {
      const isDegraded = summary.rejected_outbox_count > 0 || conflicts.length > 0;
      const runtimeState: SyncRuntimeState = isDegraded ? "degraded" : "idle";
      const finishedAt = nowIso();
      const durationMs = Date.now() - startedAtMs;
      const timeline = pushTimeline(state.timeline, {
        correlationId,
        severity: isDegraded ? "warning" : "info",
        mode,
        event: "run_completed",
        message: isDegraded
          ? "Sync cycle completed with operator attention required."
          : "Sync cycle completed successfully.",
        details: {
          pending_outbox_count: summary.pending_outbox_count,
          rejected_outbox_count: summary.rejected_outbox_count,
          unresolved_conflicts: conflicts.length,
          replay_runs_observed: replayRuns.length,
          duration_ms: durationMs,
        },
      });
      const retry: RetryState = {
        attempt: 0,
        nextRetryAt: null,
        lastError: null,
      };
      reflectAppSyncState(runtimeState, summary.pending_outbox_count, finishedAt, null, null, retry.attempt);
      return {
        runtimeState,
        blockerReason: null,
        checkpointToken: summary.checkpoint?.checkpoint_token ?? null,
        pendingBacklog: summary.pending_outbox_count,
        unresolvedConflicts: conflicts.length,
        conflictInbox: conflicts,
        replayHistoryCount: replayRuns.length,
        lastRunFinishedAt: finishedAt,
        lastSuccessAt: finishedAt,
        lastError: null,
        retry,
        timeline,
        telemetry: {
          ...state.telemetry,
          runsTotal: state.telemetry.runsTotal + 1,
          runsSucceeded: state.telemetry.runsSucceeded + 1,
          totalDurationMs: state.telemetry.totalDurationMs + durationMs,
        },
      };
    });
    useSyncOrchestratorStore.getState().scheduleNext();
  } catch (error) {
    const message = redactSecrets(toErrorMessage(error));
    useSyncOrchestratorStore.setState((state) => {
      const nextAttempt = state.retry.attempt + 1;
      const canRetry = shouldRetry(nextAttempt, state.retryPolicy);
      const retryDelayMs = canRetry ? computeRetryDelayMs(nextAttempt, state.retryPolicy) : 0;
      const nextRetryAt = canRetry ? new Date(Date.now() + retryDelayMs).toISOString() : null;
      const runtimeState: SyncRuntimeState = canRetry ? "degraded" : "error";
      const timeline = pushTimeline(state.timeline, {
        correlationId,
        severity: "error",
        mode,
        event: canRetry ? "run_retry_scheduled" : "run_failed",
        message: canRetry
          ? `Sync cycle failed; retry scheduled in ${Math.round(retryDelayMs / 1000)}s.`
          : "Sync cycle failed; retry budget exhausted.",
        details: {
          attempt: nextAttempt,
          max_attempts: state.retryPolicy.maxAttempts,
          error: message,
        },
      });
      reflectAppSyncState(runtimeState, state.pendingBacklog, state.lastSuccessAt, message, null, nextAttempt);
      return {
        runtimeState,
        lastRunFinishedAt: nowIso(),
        lastError: message,
        retry: {
          attempt: nextAttempt,
          nextRetryAt,
          lastError: message,
        },
        timeline,
        telemetry: {
          ...state.telemetry,
          runsTotal: state.telemetry.runsTotal + 1,
          runsFailed: state.telemetry.runsFailed + 1,
          retriesScheduled: state.telemetry.retriesScheduled + (canRetry ? 1 : 0),
        },
      };
    });
    const afterError = useSyncOrchestratorStore.getState();
    if (afterError.retry.nextRetryAt) {
      const retryMs = Math.max(0, new Date(afterError.retry.nextRetryAt).getTime() - Date.now());
      afterError.scheduleNext(retryMs);
    }
  }
}

export const useSyncOrchestratorStore = create<SyncOrchestratorState>()(
  persist(
    (set, get) => ({
      initialized: false,
      runtimeState: "idle",
      activeMode: "background",
      blockerReason: null,
      lastSuccessAt: null,
      lastRunStartedAt: null,
      lastRunFinishedAt: null,
      nextScheduledAt: null,
      checkpointToken: null,
      pendingBacklog: 0,
      unresolvedConflicts: 0,
      lastError: null,
      offlineSince: null,
      policy: defaultSyncPolicyControls(),
      retryPolicy: defaultRetryPolicy(),
      retry: {
        attempt: 0,
        nextRetryAt: null,
        lastError: null,
      },
      timeline: [],
      telemetry: {
        runsTotal: 0,
        runsSucceeded: 0,
        runsFailed: 0,
        runsBlocked: 0,
        retriesScheduled: 0,
        totalDurationMs: 0,
      },
      conflictInbox: [],
      replayHistoryCount: 0,

      initialize: () => {
        if (get().initialized) return;
        set((state) => ({
          initialized: true,
          runtimeState: normalizeRuntimeStateAfterRestart(state.runtimeState),
          timeline: pushTimeline(state.timeline, {
            correlationId: newCorrelationId(),
            severity: "info",
            mode: "bootstrap_restore",
            event: "orchestrator_initialized",
            message: "Sync orchestrator initialized from persisted state.",
          }),
        }));
        if (typeof window !== "undefined" && !onlineHandlerBound) {
          window.addEventListener("online", () => {
            useAppStore.getState().setOnline(true);
            set({ offlineSince: null });
            void get().runSyncNow("heartbeat_refresh");
          });
          window.addEventListener("offline", () => {
            useAppStore.getState().setOnline(false);
            set((state) => ({
              offlineSince: state.offlineSince ?? nowIso(),
            }));
          });
          onlineHandlerBound = true;
        }
        clearHeartbeatTimer();
        heartbeatTimer = setInterval(() => {
          void get().runSyncNow("heartbeat_refresh");
        }, Math.max(15, get().policy.heartbeatIntervalSeconds) * 1000);
        void get().runSyncNow("bootstrap_restore");
      },

      shutdown: () => {
        clearSchedulerTimer();
        clearHeartbeatTimer();
        set((state) => ({
          initialized: false,
          runtimeState: "paused",
          nextScheduledAt: null,
          timeline: pushTimeline(state.timeline, {
            correlationId: newCorrelationId(),
            severity: "info",
            mode: "background",
            event: "orchestrator_shutdown",
            message: "Sync orchestrator shutdown gracefully.",
          }),
        }));
      },

      scheduleNext: (delayMs?: number) => {
        clearSchedulerTimer();
        const policyDelayMs = Math.max(10, get().policy.schedulerIntervalSeconds) * 1000;
        const effectiveDelayMs = Math.max(500, delayMs ?? policyDelayMs);
        const nextAt = new Date(Date.now() + effectiveDelayMs).toISOString();
        set((state) => {
          const timeline = pushTimeline(state.timeline, {
            correlationId: newCorrelationId(),
            severity: "info",
            mode: "background",
            event: "scheduled",
            message: `Next sync scheduled in ${Math.round(effectiveDelayMs / 1000)}s.`,
            details: { next_scheduled_at: nextAt },
          });
          return {
            runtimeState: "scheduled",
            nextScheduledAt: nextAt,
            timeline,
          };
        });
        schedulerTimer = setTimeout(() => {
          void executeSyncRun("background");
        }, effectiveDelayMs);
      },

      runSyncNow: async (mode = "manual") => {
        if (!get().initialized && mode !== "bootstrap_restore") {
          get().initialize();
        }
        await executeSyncRun(mode);
      },

      runReplayRecovery: async (scope = {}) => {
        const correlationId = newCorrelationId();
        const replayKey = `replay-${Date.now()}`;
        const defaultScope: ReplaySyncFailuresInput = {
          replay_key: replayKey,
          mode: "single_item",
          reason: "Operator-triggered replay recovery",
          conflict_id: get().conflictInbox[0]?.id ?? null,
          outbox_id: get().conflictInbox[0]?.linked_outbox_id ?? null,
          server_batch_id: null,
          window_start: null,
          window_end: null,
          checkpoint_token: null,
        };
        const payload: ReplaySyncFailuresInput = { ...defaultScope, ...scope };
        try {
          const result = await replaySyncFailures(payload);
          set((state) => ({
            runtimeState: "scheduled",
            timeline: pushTimeline(state.timeline, {
              correlationId,
              severity: "info",
              mode: "replay_recovery",
              event: "replay_completed",
              message: "Replay recovery completed successfully.",
              details: {
                replay_run_id: result.run.id,
                requeued_outbox_count: result.requeued_outbox_count,
                transitioned_conflict_count: result.transitioned_conflict_count,
                checkpoint_token_after: result.checkpoint_token_after,
              },
            }),
          }));
          await get().refreshInbox();
          get().scheduleNext(500);
        } catch (error) {
          const message = redactSecrets(toErrorMessage(error));
          set((state) => ({
            runtimeState: "degraded",
            lastError: message,
            timeline: pushTimeline(state.timeline, {
              correlationId,
              severity: "error",
              mode: "replay_recovery",
              event: "replay_failed",
              message: "Replay recovery failed.",
              details: { error: message },
            }),
          }));
        }
      },

      resolveConflictAction: async (input) => {
        const correlationId = newCorrelationId();
        const resolved = await resolveSyncConflict(input);
        set((state) => ({
          conflictInbox: state.conflictInbox.map((item) => (item.id === resolved.id ? resolved : item)),
          timeline: pushTimeline(state.timeline, {
            correlationId,
            severity: "info",
            mode: "manual",
            event: "conflict_resolved",
            message: `Conflict ${resolved.id} moved to ${resolved.status}.`,
            details: {
              conflict_id: resolved.id,
              action: input.action,
              status: resolved.status,
            },
          }),
        }));
        await get().refreshInbox();
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
        set((state) => ({
          conflictInbox: conflicts,
          replayHistoryCount: replayRuns.length,
          unresolvedConflicts: conflicts.length,
          pendingBacklog: summary.pending_outbox_count,
          checkpointToken: summary.checkpoint?.checkpoint_token ?? null,
          runtimeState:
            state.runtimeState === "error"
              ? "error"
              : conflicts.length > 0 || summary.rejected_outbox_count > 0
                ? "degraded"
                : state.runtimeState,
        }));
      },

      updatePolicy: (patch) => {
        set((state) => ({
          policy: {
            ...state.policy,
            ...patch,
          },
        }));
      },

      pauseForMinutes: (minutes) => {
        const bounded = Math.max(1, Math.min(24 * 60, minutes));
        const pausedUntil = new Date(Date.now() + bounded * 60_000).toISOString();
        set((state) => ({
          policy: { ...state.policy, pausedUntil },
          runtimeState: "paused",
          blockerReason: `Sync paused until ${pausedUntil}.`,
          timeline: pushTimeline(state.timeline, {
            correlationId: newCorrelationId(),
            severity: "warning",
            mode: "manual",
            event: "sync_paused",
            message: `Sync paused for ${bounded} minute(s).`,
          }),
        }));
      },

      resumeNow: () => {
        set((state) => ({
          policy: { ...state.policy, pausedUntil: null },
          runtimeState: "scheduled",
          blockerReason: null,
          timeline: pushTimeline(state.timeline, {
            correlationId: newCorrelationId(),
            severity: "info",
            mode: "manual",
            event: "sync_resumed",
            message: "Sync resumed by operator.",
          }),
        }));
        void get().runSyncNow("manual");
      },

      exportDiagnostics: () => {
        const state = get();
        const avgDuration =
          state.telemetry.runsSucceeded > 0
            ? Math.round(state.telemetry.totalDurationMs / state.telemetry.runsSucceeded)
            : 0;
        const timeline = state.timeline
          .slice(-120)
          .map((event) => ({
            ...event,
            message: redactSecrets(event.message),
            details:
              event.details === undefined
                ? undefined
                : JSON.parse(redactSecrets(JSON.stringify(event.details))),
          }));
        return {
          generatedAt: nowIso(),
          machineContext: {
            userAgent: typeof navigator === "undefined" ? "unknown" : navigator.userAgent,
            language: typeof navigator === "undefined" ? "unknown" : navigator.language,
            platform: typeof navigator === "undefined" ? "unknown" : navigator.platform,
            online: typeof navigator === "undefined" ? true : navigator.onLine,
          },
          runtime: {
            state: state.runtimeState,
            mode: state.activeMode,
            blockerReason: state.blockerReason,
            checkpointToken: state.checkpointToken,
            pendingBacklog: state.pendingBacklog,
            unresolvedConflicts: state.unresolvedConflicts,
            lastSuccessAt: state.lastSuccessAt,
            nextScheduledAt: state.nextScheduledAt,
          },
          policy: state.policy,
          retry: state.retry,
          telemetry: {
            ...state.telemetry,
            averageDurationMs: avgDuration,
          },
          timeline,
        };
      },
    }),
    {
      name: "maintafox-sync-orchestrator",
      partialize: (state) => ({
        runtimeState: state.runtimeState,
        activeMode: state.activeMode,
        blockerReason: state.blockerReason,
        lastSuccessAt: state.lastSuccessAt,
        lastRunStartedAt: state.lastRunStartedAt,
        lastRunFinishedAt: state.lastRunFinishedAt,
        nextScheduledAt: state.nextScheduledAt,
        checkpointToken: state.checkpointToken,
        pendingBacklog: state.pendingBacklog,
        unresolvedConflicts: state.unresolvedConflicts,
        lastError: state.lastError,
        offlineSince: state.offlineSince,
        policy: state.policy,
        retryPolicy: state.retryPolicy,
        retry: state.retry,
        timeline: state.timeline,
        telemetry: state.telemetry,
        replayHistoryCount: state.replayHistoryCount,
      }),
    },
  ),
);

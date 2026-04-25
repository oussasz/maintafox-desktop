export type SyncRuntimeState =
  | "idle"
  | "scheduled"
  | "running"
  | "blocked"
  | "degraded"
  | "error"
  | "paused";

export type SyncRunMode =
  | "background"
  | "manual"
  | "bootstrap_restore"
  | "replay_recovery"
  | "heartbeat_refresh";

export type SyncEntitlementStatus = "active" | "suspended" | "blocked";

export interface SyncPolicyControls {
  entitlementStatus: SyncEntitlementStatus;
  pausedUntil: string | null;
  offlineGraceSeconds: number;
  bandwidthMode: "normal" | "constrained";
  heartbeatIntervalSeconds: number;
  schedulerIntervalSeconds: number;
}

export interface SchedulingContext {
  nowIso: string;
  isOnline: boolean;
  offlineSinceIso: string | null;
}

export interface SchedulingGate {
  allowed: boolean;
  state: SyncRuntimeState;
  blockerReason: string | null;
}

export interface RetryPolicy {
  baseDelayMs: number;
  maxDelayMs: number;
  maxAttempts: number;
  jitterRatio: number;
}

export function defaultSyncPolicyControls(): SyncPolicyControls {
  return {
    entitlementStatus: "active",
    pausedUntil: null,
    offlineGraceSeconds: 300,
    bandwidthMode: "normal",
    heartbeatIntervalSeconds: 60,
    schedulerIntervalSeconds: 90,
  };
}

export function defaultRetryPolicy(): RetryPolicy {
  return {
    baseDelayMs: 1_500,
    maxDelayMs: 120_000,
    maxAttempts: 8,
    jitterRatio: 0.25,
  };
}

/** After this many ms since the last failed sync run, automatic modes may refresh an exhausted retry budget. */
export const SYNC_EXHAUSTED_RETRY_COOLDOWN_MS = 120_000;

export function computeRetryDelayMs(
  attempt: number,
  policy: RetryPolicy,
  randomFn: () => number = Math.random,
): number {
  const boundedAttempt = Math.max(1, attempt);
  const exp = Math.min(policy.maxDelayMs, policy.baseDelayMs * 2 ** (boundedAttempt - 1));
  const jitterBand = Math.max(0, Math.floor(exp * policy.jitterRatio));
  const jitter = jitterBand === 0 ? 0 : Math.floor((randomFn() * 2 - 1) * jitterBand);
  return Math.max(policy.baseDelayMs, exp + jitter);
}

export function evaluateSchedulingGate(
  policy: SyncPolicyControls,
  context: SchedulingContext,
): SchedulingGate {
  const now = new Date(context.nowIso);
  if (policy.entitlementStatus === "suspended") {
    return {
      allowed: false,
      state: "blocked",
      blockerReason: "Sync suspended by entitlement policy.",
    };
  }
  if (policy.entitlementStatus === "blocked") {
    return {
      allowed: false,
      state: "blocked",
      blockerReason: "Sync blocked by entitlement policy.",
    };
  }
  if (policy.pausedUntil) {
    const pausedUntil = new Date(policy.pausedUntil);
    if (pausedUntil.getTime() > now.getTime()) {
      return {
        allowed: false,
        state: "paused",
        blockerReason: `Sync paused until ${pausedUntil.toISOString()}.`,
      };
    }
  }
  if (!context.isOnline) {
    const offlineSince = context.offlineSinceIso ? new Date(context.offlineSinceIso) : now;
    const offlineDurationSec = Math.max(0, (now.getTime() - offlineSince.getTime()) / 1000);
    if (offlineDurationSec > policy.offlineGraceSeconds) {
      return {
        allowed: false,
        state: "blocked",
        blockerReason: "Network offline beyond grace period.",
      };
    }
    return {
      allowed: false,
      state: "degraded",
      blockerReason: "Network currently offline (within grace period).",
    };
  }
  return { allowed: true, state: "scheduled", blockerReason: null };
}

export function shouldRetry(attempt: number, policy: RetryPolicy): boolean {
  return attempt < policy.maxAttempts;
}

export function normalizeRuntimeStateAfterRestart(state: SyncRuntimeState): SyncRuntimeState {
  if (state === "running") {
    return "scheduled";
  }
  // Persisted "error" is terminal for the last session; on cold start give a fresh retry cycle
  // instead of showing the header error until the next successful round.
  if (state === "error") {
    return "idle";
  }
  return state;
}

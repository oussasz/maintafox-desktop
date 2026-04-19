import { describe, expect, it } from "vitest";

import {
  computeRetryDelayMs,
  defaultRetryPolicy,
  defaultSyncPolicyControls,
  evaluateSchedulingGate,
  normalizeRuntimeStateAfterRestart,
  shouldRetry,
} from "@/services/sync-orchestrator-core";

describe("sync orchestrator core", () => {
  it("computes bounded retry delay with deterministic jitter", () => {
    const policy = defaultRetryPolicy();
    const delay = computeRetryDelayMs(3, policy, () => 0.5);
    expect(delay).toBeGreaterThanOrEqual(policy.baseDelayMs);
    expect(delay).toBeLessThanOrEqual(policy.maxDelayMs);
  });

  it("blocks scheduling when entitlement is suspended", () => {
    const policy = defaultSyncPolicyControls();
    policy.entitlementStatus = "suspended";
    const gate = evaluateSchedulingGate(policy, {
      nowIso: "2026-01-01T00:00:00.000Z",
      isOnline: true,
      offlineSinceIso: null,
    });
    expect(gate.allowed).toBe(false);
    expect(gate.state).toBe("blocked");
    expect(gate.blockerReason).toContain("suspended");
  });

  it("returns degraded while offline within grace period", () => {
    const policy = defaultSyncPolicyControls();
    policy.offlineGraceSeconds = 120;
    const gate = evaluateSchedulingGate(policy, {
      nowIso: "2026-01-01T00:01:00.000Z",
      isOnline: false,
      offlineSinceIso: "2026-01-01T00:00:10.000Z",
    });
    expect(gate.allowed).toBe(false);
    expect(gate.state).toBe("degraded");
  });

  it("blocks when offline exceeds grace period", () => {
    const policy = defaultSyncPolicyControls();
    policy.offlineGraceSeconds = 10;
    const gate = evaluateSchedulingGate(policy, {
      nowIso: "2026-01-01T00:01:00.000Z",
      isOnline: false,
      offlineSinceIso: "2026-01-01T00:00:00.000Z",
    });
    expect(gate.allowed).toBe(false);
    expect(gate.state).toBe("blocked");
  });

  it("prevents retries after max attempts (network chaos stop condition)", () => {
    const policy = defaultRetryPolicy();
    expect(shouldRetry(policy.maxAttempts - 1, policy)).toBe(true);
    expect(shouldRetry(policy.maxAttempts, policy)).toBe(false);
  });

  it("normalizes runtime state after restart", () => {
    expect(normalizeRuntimeStateAfterRestart("running")).toBe("scheduled");
    expect(normalizeRuntimeStateAfterRestart("degraded")).toBe("degraded");
    expect(normalizeRuntimeStateAfterRestart("error")).toBe("idle");
    expect(normalizeRuntimeStateAfterRestart("idle")).toBe("idle");
  });
});

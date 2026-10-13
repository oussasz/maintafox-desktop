/**
 * settings-store — behaviour tests.
 *
 * Mocks only the settings-service boundary (`getSessionPolicy`).
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { getSessionPolicy } from "@/services/settings-service";
import type { PolicySnapshot, PolicyTestResult, SessionPolicy } from "@shared/ipc-types";

import { useSettingsStore } from "../settings-store";

vi.mock("@/services/settings-service", () => ({
  getSessionPolicy: vi.fn(),
}));

const mockGetSessionPolicy = vi.mocked(getSessionPolicy);

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makePolicy(overrides: Partial<SessionPolicy> = {}): SessionPolicy {
  return {
    idle_timeout_minutes: 15,
    absolute_session_minutes: 480,
    offline_grace_hours: 72,
    step_up_window_minutes: 5,
    max_failed_attempts: 5,
    lockout_minutes: 15,
    ...overrides,
  };
}

function makeSnapshot(overrides: Partial<PolicySnapshot> = {}): PolicySnapshot {
  return {
    id: 7,
    policy_domain: "session",
    version_no: 2,
    snapshot_json: '{"idle_timeout_minutes":30}',
    is_active: false,
    activated_at: null,
    activated_by_id: null,
    ...overrides,
  };
}

function makeTestResult(overrides: Partial<PolicyTestResult> = {}): PolicyTestResult {
  return {
    rule_name: "idle_timeout_range",
    severity: "pass",
    message: "Idle timeout within range",
    ...overrides,
  };
}

const initialState = useSettingsStore.getState();

function resetStore() {
  useSettingsStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockGetSessionPolicy.mockReset();
  resetStore();
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe("settings-store", () => {
  it("starts with an empty, idle state", () => {
    const s = useSettingsStore.getState();
    expect(s.sessionPolicy).toBeNull();
    expect(s.policyLoading).toBe(false);
    expect(s.policyError).toBeNull();
    expect(s.activeCategory).toBeNull();
    expect(s.editingPolicy).toBeNull();
    expect(s.draftPolicy).toBeNull();
    expect(s.testResults).toBeNull();
    expect(s.policyOperationLoading).toBe(false);
  });

  describe("loadSessionPolicy", () => {
    it("sets policyLoading while in flight, then stores the policy", async () => {
      let resolve: (p: SessionPolicy) => void = () => undefined;
      mockGetSessionPolicy.mockReturnValueOnce(
        new Promise<SessionPolicy>((r) => {
          resolve = r;
        }),
      );
      useSettingsStore.setState({ policyError: "stale" });

      const pending = useSettingsStore.getState().loadSessionPolicy();
      expect(useSettingsStore.getState().policyLoading).toBe(true);
      expect(useSettingsStore.getState().policyError).toBeNull();

      const policy = makePolicy({ idle_timeout_minutes: 42 });
      resolve(policy);
      await pending;

      const s = useSettingsStore.getState();
      expect(mockGetSessionPolicy).toHaveBeenCalledTimes(1);
      expect(mockGetSessionPolicy).toHaveBeenCalledWith();
      expect(s.policyLoading).toBe(false);
      expect(s.sessionPolicy).toEqual(policy);
      expect(s.policyError).toBeNull();
    });

    it("captures the error message and does not throw on failure", async () => {
      const previous = makePolicy();
      useSettingsStore.setState({ sessionPolicy: previous });
      mockGetSessionPolicy.mockRejectedValueOnce(new Error("bridge unavailable"));

      await expect(useSettingsStore.getState().loadSessionPolicy()).resolves.toBeUndefined();

      const s = useSettingsStore.getState();
      expect(s.policyLoading).toBe(false);
      expect(s.policyError).toBe("bridge unavailable");
      // Previously cached policy is kept on failure.
      expect(s.sessionPolicy).toEqual(previous);
      expect(console.warn).toHaveBeenCalled();
    });

    it("stringifies non-Error rejections", async () => {
      mockGetSessionPolicy.mockRejectedValueOnce("plain failure");

      await useSettingsStore.getState().loadSessionPolicy();

      expect(useSettingsStore.getState().policyError).toBe("plain failure");
    });
  });

  it("applySessionPolicy replaces the cached policy without IPC", () => {
    useSettingsStore.setState({ sessionPolicy: makePolicy() });
    const next = makePolicy({ idle_timeout_minutes: 60, lockout_minutes: 30 });

    useSettingsStore.getState().applySessionPolicy(next);

    expect(useSettingsStore.getState().sessionPolicy).toEqual(next);
    expect(mockGetSessionPolicy).not.toHaveBeenCalled();
  });

  it("setActiveCategory sets and clears the active category", () => {
    useSettingsStore.getState().setActiveCategory("security");
    expect(useSettingsStore.getState().activeCategory).toBe("security");

    useSettingsStore.getState().setActiveCategory(null);
    expect(useSettingsStore.getState().activeCategory).toBeNull();
  });

  describe("policy editor", () => {
    it("openPolicyEditor sets the editing domain only", () => {
      const draft = makeSnapshot();
      useSettingsStore.setState({ draftPolicy: draft });

      useSettingsStore.getState().openPolicyEditor("backup");

      const s = useSettingsStore.getState();
      expect(s.editingPolicy).toBe("backup");
      expect(s.draftPolicy).toEqual(draft);
    });

    it("closePolicyEditor clears domain, draft and test results", () => {
      useSettingsStore.setState({
        editingPolicy: "session",
        draftPolicy: makeSnapshot(),
        testResults: [makeTestResult()],
        policyOperationLoading: true,
        activeCategory: "security",
      });

      useSettingsStore.getState().closePolicyEditor();

      const s = useSettingsStore.getState();
      expect(s.editingPolicy).toBeNull();
      expect(s.draftPolicy).toBeNull();
      expect(s.testResults).toBeNull();
      // Unrelated state is untouched.
      expect(s.policyOperationLoading).toBe(true);
      expect(s.activeCategory).toBe("security");
    });

    it("setDraftPolicy stores and clears a draft snapshot", () => {
      const draft = makeSnapshot({ version_no: 3 });
      useSettingsStore.getState().setDraftPolicy(draft);
      expect(useSettingsStore.getState().draftPolicy).toEqual(draft);

      useSettingsStore.getState().setDraftPolicy(null);
      expect(useSettingsStore.getState().draftPolicy).toBeNull();
    });

    it("setTestResults stores and clears validation results", () => {
      const results = [
        makeTestResult(),
        makeTestResult({ rule_name: "lockout", severity: "fail", message: "Too short" }),
      ];
      useSettingsStore.getState().setTestResults(results);
      expect(useSettingsStore.getState().testResults).toEqual(results);

      useSettingsStore.getState().setTestResults(null);
      expect(useSettingsStore.getState().testResults).toBeNull();
    });

    it("setPolicyOperationLoading toggles the operation flag", () => {
      useSettingsStore.getState().setPolicyOperationLoading(true);
      expect(useSettingsStore.getState().policyOperationLoading).toBe(true);

      useSettingsStore.getState().setPolicyOperationLoading(false);
      expect(useSettingsStore.getState().policyOperationLoading).toBe(false);
    });
  });
});

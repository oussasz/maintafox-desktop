/**
 * updater-store — behaviour tests.
 *
 * Mocks only the service boundary: updater-service and the licence onboarding
 * lookup from product-license-service.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  type ProductLicenseOnboardingState,
  getProductLicenseOnboardingState,
} from "@/services/product-license-service";
import { checkForUpdate, installPendingUpdate } from "@/services/updater-service";
import type { UpdateCheckResult } from "@shared/ipc-types";

import { useUpdaterStore } from "../updater-store";

vi.mock("@/services/updater-service", () => ({
  checkForUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
}));

vi.mock("@/services/product-license-service", () => ({
  getProductLicenseOnboardingState: vi.fn(),
}));

const mockCheck = vi.mocked(checkForUpdate);
const mockInstall = vi.mocked(installPendingUpdate);
const mockOnboarding = vi.mocked(getProductLicenseOnboardingState);

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeCheck(overrides: Partial<UpdateCheckResult> = {}): UpdateCheckResult {
  return {
    available: true,
    version: "1.2.0",
    notes: "Bug fixes",
    pub_date: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

function makeOnboarding(
  overrides: Partial<ProductLicenseOnboardingState> = {},
): ProductLicenseOnboardingState {
  return {
    complete: true,
    status: "active",
    pending_online_validation: false,
    deny_reason_code: null,
    deny_message: null,
    ...overrides,
  };
}

const initialState = useUpdaterStore.getState();

function resetStore() {
  useUpdaterStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCheck.mockReset();
  mockInstall.mockReset();
  mockOnboarding.mockReset();
  resetStore();
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe("updater-store", () => {
  it("starts idle with no result", () => {
    const s = useUpdaterStore.getState();
    expect(s.lastCheckResult).toBeNull();
    expect(s.isChecking).toBe(false);
    expect(s.isInstalling).toBe(false);
    expect(s.installComplete).toBe(false);
    expect(s.forceRequired).toBe(false);
    expect(s.forceReason).toBeNull();
    expect(s.error).toBeNull();
  });

  describe("checkForUpdate", () => {
    it("sets isChecking while in flight and stores the result", async () => {
      let resolve: (r: UpdateCheckResult) => void = () => undefined;
      mockCheck.mockReturnValueOnce(
        new Promise<UpdateCheckResult>((r) => {
          resolve = r;
        }),
      );
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());
      useUpdaterStore.setState({ error: "old error" });

      const pending = useUpdaterStore.getState().checkForUpdate();
      expect(useUpdaterStore.getState().isChecking).toBe(true);
      expect(useUpdaterStore.getState().error).toBeNull();

      const result = makeCheck();
      resolve(result);
      await pending;

      const s = useUpdaterStore.getState();
      expect(mockCheck).toHaveBeenCalledTimes(1);
      expect(mockOnboarding).toHaveBeenCalledTimes(1);
      expect(s.isChecking).toBe(false);
      expect(s.lastCheckResult).toEqual(result);
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBeNull();
      expect(s.error).toBeNull();
    });

    it("stores a no-update result without error", async () => {
      const result = makeCheck({ available: false, version: null, notes: null, pub_date: null });
      mockCheck.mockResolvedValueOnce(result);
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());

      await useUpdaterStore.getState().checkForUpdate();

      expect(useUpdaterStore.getState().lastCheckResult).toEqual(result);
      expect(useUpdaterStore.getState().error).toBeNull();
    });

    it("flags a forced update when an update package is available (no error)", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck());
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({
          status: "denied_force_update_required",
          deny_message: "Version 1.1 is no longer supported",
        }),
      );

      await useUpdaterStore.getState().checkForUpdate();

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(true);
      expect(s.forceReason).toBe("Version 1.1 is no longer supported");
      expect(s.error).toBeNull();
    });

    it("uses the deny message as the error when forced but no package is available", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck({ available: false, version: null }));
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({
          status: "denied_force_update_required",
          deny_message: "Update required by your administrator",
        }),
      );

      await useUpdaterStore.getState().checkForUpdate();

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(true);
      expect(s.error).toBe("Update required by your administrator");
    });

    it("falls back to a generic error when forced, no package and no deny message", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck({ available: false, version: null }));
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({ status: "denied_force_update_required", deny_message: undefined }),
      );

      await useUpdaterStore.getState().checkForUpdate();

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(true);
      expect(s.forceReason).toBeNull();
      expect(s.error).toBe(
        "A forced update policy is active but no update package is currently available.",
      );
    });

    it("carries the deny message as forceReason even when not forced", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck());
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({ status: "denied_expired", deny_message: "Licence expired" }),
      );

      await useUpdaterStore.getState().checkForUpdate();

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBe("Licence expired");
      expect(s.error).toBeNull();
    });

    it("treats an onboarding lookup failure as not forced", async () => {
      const result = makeCheck();
      mockCheck.mockResolvedValueOnce(result);
      mockOnboarding.mockRejectedValueOnce(new Error("no session"));
      useUpdaterStore.setState({ forceRequired: true, forceReason: "old" });

      await useUpdaterStore.getState().checkForUpdate();

      const s = useUpdaterStore.getState();
      expect(s.lastCheckResult).toEqual(result);
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBeNull();
      expect(s.error).toBeNull();
    });

    it("captures check failure, clears force state, keeps the previous result, does not throw", async () => {
      const previous = makeCheck({ version: "1.1.0" });
      useUpdaterStore.setState({
        lastCheckResult: previous,
        forceRequired: true,
        forceReason: "old reason",
      });
      mockCheck.mockRejectedValueOnce(new Error("IPC bridge down"));
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());

      await expect(useUpdaterStore.getState().checkForUpdate()).resolves.toBeUndefined();

      const s = useUpdaterStore.getState();
      expect(s.isChecking).toBe(false);
      expect(s.error).toBe("IPC bridge down");
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBeNull();
      expect(s.lastCheckResult).toEqual(previous);
      expect(console.warn).toHaveBeenCalled();
    });
  });

  describe("installUpdate", () => {
    it("sets isInstalling while in flight, then marks installComplete", async () => {
      let resolve: () => void = () => undefined;
      mockInstall.mockReturnValueOnce(
        new Promise<void>((r) => {
          resolve = r;
        }),
      );
      useUpdaterStore.setState({ error: "old error" });

      const pending = useUpdaterStore.getState().installUpdate();
      expect(useUpdaterStore.getState().isInstalling).toBe(true);
      expect(useUpdaterStore.getState().error).toBeNull();
      expect(useUpdaterStore.getState().installComplete).toBe(false);

      resolve();
      await pending;

      const s = useUpdaterStore.getState();
      expect(mockInstall).toHaveBeenCalledTimes(1);
      expect(s.isInstalling).toBe(false);
      expect(s.installComplete).toBe(true);
      expect(s.error).toBeNull();
    });

    it("captures install failure without throwing", async () => {
      mockInstall.mockRejectedValueOnce(new Error("signature mismatch"));

      await expect(useUpdaterStore.getState().installUpdate()).resolves.toBeUndefined();

      const s = useUpdaterStore.getState();
      expect(s.isInstalling).toBe(false);
      expect(s.installComplete).toBe(false);
      expect(s.error).toBe("signature mismatch");
    });
  });

  it("dismissNotification clears result, error, completion and force flags", () => {
    useUpdaterStore.setState({
      lastCheckResult: makeCheck(),
      error: "boom",
      installComplete: true,
      forceRequired: true,
      forceReason: "policy",
      isChecking: true,
      isInstalling: true,
    });

    useUpdaterStore.getState().dismissNotification();

    const s = useUpdaterStore.getState();
    expect(s.lastCheckResult).toBeNull();
    expect(s.error).toBeNull();
    expect(s.installComplete).toBe(false);
    expect(s.forceRequired).toBe(false);
    expect(s.forceReason).toBeNull();
    // In-flight flags are not touched by dismiss.
    expect(s.isChecking).toBe(true);
    expect(s.isInstalling).toBe(true);
  });
});

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
import {
  type UpdateProgressHandler,
  acknowledgeUpdateNotice,
  checkForUpdate,
  getUpdateJournal,
  installPendingUpdate,
  reinstallPreviousVersion,
} from "@/services/updater-service";
import type { InstallOutcome, UpdateCheckResult, UpdateJournalView } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  isOfferVisible,
  isUpdateMandatory,
  pendingUpdateNotice,
  useUpdaterStore,
} from "../updater-store";

vi.mock("@/services/updater-service", () => ({
  checkForUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
  reinstallPreviousVersion: vi.fn(),
  getUpdateJournal: vi.fn(),
  acknowledgeUpdateNotice: vi.fn(),
}));

vi.mock("@/services/product-license-service", () => ({
  getProductLicenseOnboardingState: vi.fn(),
}));

const mockCheck = vi.mocked(checkForUpdate);
const mockInstall = vi.mocked(installPendingUpdate);
const mockReinstall = vi.mocked(reinstallPreviousVersion);
const mockJournal = vi.mocked(getUpdateJournal);
const mockAcknowledge = vi.mocked(acknowledgeUpdateNotice);
const mockOnboarding = vi.mocked(getProductLicenseOnboardingState);

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeCheck(overrides: Partial<UpdateCheckResult> = {}): UpdateCheckResult {
  return {
    status: "available",
    available: true,
    current_version: "1.1.0",
    version: "1.2.0",
    notes: "Bug fixes",
    pub_date: "2026-10-01T00:00:00Z",
    kind: "upgrade",
    mandatory: false,
    mandatory_reason: null,
    mandatory_message: null,
    notes_en: "Bug fixes",
    notes_fr: "Corrections",
    message: null,
    ...overrides,
  };
}

function makeJournal(overrides: Partial<UpdateJournalView> = {}): UpdateJournalView {
  return {
    state: "rolled_back",
    kind: "rollback",
    from_version: "1.2.0",
    to_version: "1.1.0",
    failure: null,
    rollback_refusal: null,
    rollback_allowed_version: null,
    notice_acknowledged: false,
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

/** An install command whose progress and outcome the test drives. */
function controllableInstall() {
  let progress: UpdateProgressHandler = () => undefined;
  let finish: (outcome: InstallOutcome) => void = () => undefined;
  let fail: (err: Error) => void = () => undefined;
  const impl = (_locale: string | undefined, onProgress: UpdateProgressHandler) => {
    progress = onProgress;
    return new Promise<InstallOutcome>((resolve, reject) => {
      finish = resolve;
      fail = reject;
    });
  };
  return {
    impl,
    progress: (e: Parameters<UpdateProgressHandler>[0]) => progress(e),
    finish: (o: InstallOutcome) => finish(o),
    fail: (e: Error) => fail(e),
  };
}

const initialState = useUpdaterStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockCheck.mockReset();
  mockInstall.mockReset();
  mockReinstall.mockReset();
  mockJournal.mockReset();
  mockAcknowledge.mockReset();
  mockOnboarding.mockReset();
  useUpdaterStore.setState(initialState, true);
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe("updater-store", () => {
  it("starts idle with no result", () => {
    const s = useUpdaterStore.getState();
    expect(s.lastCheckResult).toBeNull();
    expect(s.isChecking).toBe(false);
    expect(s.phase).toBe("idle");
    expect(s.refusal).toBeNull();
    expect(s.error).toBeNull();
    expect(s.forceRequired).toBe(false);
    expect(s.journal).toBeNull();
  });

  describe("check", () => {
    it("sets isChecking while in flight, passes the locale and stores the result", async () => {
      let resolve: (r: UpdateCheckResult) => void = () => undefined;
      mockCheck.mockReturnValueOnce(
        new Promise<UpdateCheckResult>((r) => {
          resolve = r;
        }),
      );
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());
      useUpdaterStore.setState({ checkError: "old error" });

      const pending = useUpdaterStore.getState().check("fr");
      expect(useUpdaterStore.getState().isChecking).toBe(true);
      expect(useUpdaterStore.getState().checkError).toBeNull();

      const result = makeCheck();
      resolve(result);
      await pending;

      const s = useUpdaterStore.getState();
      expect(mockCheck).toHaveBeenCalledWith("fr");
      expect(s.isChecking).toBe(false);
      expect(s.lastCheckResult).toEqual(result);
      expect(s.forceRequired).toBe(false);
    });

    it("keeps not_configured as a calm status, not an error", async () => {
      const result = makeCheck({
        status: "not_configured",
        available: false,
        version: null,
        kind: null,
      });
      mockCheck.mockResolvedValueOnce(result);
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());

      await useUpdaterStore.getState().check("en");

      const s = useUpdaterStore.getState();
      expect(s.lastCheckResult?.status).toBe("not_configured");
      expect(s.checkError).toBeNull();
      expect(s.error).toBeNull();
      expect(isOfferVisible(s)).toBe(false);
    });

    it("flags the licence forced-update policy with its reason", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck());
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({
          status: "denied_force_update_required",
          deny_message: "Version 1.1 is no longer supported",
        }),
      );

      await useUpdaterStore.getState().check("en");

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(true);
      expect(s.forceReason).toBe("Version 1.1 is no longer supported");
      expect(isUpdateMandatory(s)).toBe(true);
    });

    it("ignores a deny message when the policy does not force an update", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck());
      mockOnboarding.mockResolvedValueOnce(
        makeOnboarding({ status: "denied_expired", deny_message: "Licence expired" }),
      );

      await useUpdaterStore.getState().check("en");

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBeNull();
    });

    it("treats an onboarding lookup failure as not forced", async () => {
      mockCheck.mockResolvedValueOnce(makeCheck());
      mockOnboarding.mockRejectedValueOnce(new Error("no session"));
      useUpdaterStore.setState({ forceRequired: true, forceReason: "old" });

      await useUpdaterStore.getState().check("en");

      const s = useUpdaterStore.getState();
      expect(s.forceRequired).toBe(false);
      expect(s.forceReason).toBeNull();
      expect(s.checkError).toBeNull();
    });

    it("captures a check failure, keeps the previous result, does not throw", async () => {
      const previous = makeCheck({ version: "1.1.5" });
      useUpdaterStore.setState({ lastCheckResult: previous });
      mockCheck.mockRejectedValueOnce(new Error("IPC bridge down"));
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());

      await expect(useUpdaterStore.getState().check("en")).resolves.toBeUndefined();

      const s = useUpdaterStore.getState();
      expect(s.isChecking).toBe(false);
      expect(s.checkError).toBe("IPC bridge down");
      expect(s.lastCheckResult).toEqual(previous);
    });
  });

  describe("mandatory and dismiss", () => {
    it("the manifest's mandatory flag is the primary signal", () => {
      expect(
        isUpdateMandatory({
          lastCheckResult: makeCheck({ mandatory: true }),
          forceRequired: false,
        }),
      ).toBe(true);
      expect(isUpdateMandatory({ lastCheckResult: makeCheck(), forceRequired: false })).toBe(false);
      // A mandatory flag without an available update means nothing to install.
      expect(
        isUpdateMandatory({
          lastCheckResult: makeCheck({ available: false, mandatory: true }),
          forceRequired: false,
        }),
      ).toBe(false);
    });

    it("hides an optional offer for that version", () => {
      useUpdaterStore.setState({ lastCheckResult: makeCheck() });

      useUpdaterStore.getState().dismiss();

      const s = useUpdaterStore.getState();
      expect(s.dismissedVersion).toBe("1.2.0");
      expect(isOfferVisible(s)).toBe(false);
      // A newer offer shows again.
      expect(isOfferVisible({ ...s, lastCheckResult: makeCheck({ version: "1.3.0" }) })).toBe(true);
    });

    it("cannot dismiss a mandatory update", () => {
      useUpdaterStore.setState({ lastCheckResult: makeCheck({ mandatory: true }) });

      useUpdaterStore.getState().dismiss();

      const s = useUpdaterStore.getState();
      expect(s.dismissedVersion).toBeNull();
      expect(isOfferVisible(s)).toBe(true);
    });

    it("cannot dismiss while the licence forces the update", () => {
      useUpdaterStore.setState({ lastCheckResult: makeCheck(), forceRequired: true });

      useUpdaterStore.getState().dismiss();

      expect(isOfferVisible(useUpdaterStore.getState())).toBe(true);
    });
  });

  describe("install", () => {
    it("goes through the progress phases and ends restarting", async () => {
      const ctl = controllableInstall();
      mockInstall.mockImplementationOnce(ctl.impl);
      useUpdaterStore.setState({ lastCheckResult: makeCheck(), error: "old" });

      const pending = useUpdaterStore.getState().install("fr");
      expect(mockInstall).toHaveBeenCalledWith("fr", expect.any(Function));
      expect(useUpdaterStore.getState().phase).toBe("starting");
      expect(useUpdaterStore.getState().error).toBeNull();

      ctl.progress({ event: "backing_up" });
      expect(useUpdaterStore.getState().phase).toBe("backing_up");
      ctl.progress({ event: "downloading", downloaded_bytes: 1_000, total_bytes: 4_000 });
      expect(useUpdaterStore.getState()).toMatchObject({
        phase: "downloading",
        downloadedBytes: 1_000,
        totalBytes: 4_000,
      });
      ctl.progress({ event: "downloading", downloaded_bytes: 2_000, total_bytes: null });
      expect(useUpdaterStore.getState().totalBytes).toBeNull();
      ctl.progress({ event: "installing" });
      expect(useUpdaterStore.getState().phase).toBe("installing");

      ctl.finish({ outcome: "restarting" });
      await pending;
      expect(useUpdaterStore.getState().phase).toBe("restarting");
    });

    it("records the restoring_backup phase of a rollback", () => {
      const ctl = controllableInstall();
      mockInstall.mockImplementationOnce(ctl.impl);

      void useUpdaterStore.getState().install("en");
      ctl.progress({ event: "restoring_backup" });

      expect(useUpdaterStore.getState().phase).toBe("restoring_backup");
    });

    it("ignores a second install while one runs", () => {
      const ctl = controllableInstall();
      mockInstall.mockImplementation(ctl.impl);

      void useUpdaterStore.getState().install("en");
      void useUpdaterStore.getState().install("en");

      expect(mockInstall).toHaveBeenCalledTimes(1);
    });

    it("maps a refusal back to idle with the refusal and target version", async () => {
      mockInstall.mockResolvedValueOnce({
        outcome: "refused",
        refusal: { code: "rollback_unsynced_changes", count: 4 },
      });
      useUpdaterStore.setState({
        lastCheckResult: makeCheck({ kind: "rollback", version: "1.0.0" }),
      });

      await useUpdaterStore.getState().install("en");

      const s = useUpdaterStore.getState();
      expect(s.phase).toBe("idle");
      expect(s.refusal).toEqual({ code: "rollback_unsynced_changes", count: 4 });
      expect(s.installTargetVersion).toBe("1.0.0");
      expect(mockCheck).not.toHaveBeenCalled();
    });

    it("re-checks after update_withdrawn so the offer is current", async () => {
      mockInstall.mockResolvedValueOnce({
        outcome: "refused",
        refusal: { code: "update_withdrawn" },
      });
      const noUpdate = makeCheck({ status: "up_to_date", available: false, version: null });
      mockCheck.mockResolvedValueOnce(noUpdate);
      mockOnboarding.mockResolvedValueOnce(makeOnboarding());
      useUpdaterStore.setState({ lastCheckResult: makeCheck() });

      await useUpdaterStore.getState().install("fr");

      const s = useUpdaterStore.getState();
      expect(s.refusal).toEqual({ code: "update_withdrawn" });
      expect(mockCheck).toHaveBeenCalledWith("fr");
      expect(s.lastCheckResult).toEqual(noUpdate);
    });

    it("captures an install failure without throwing", async () => {
      mockInstall.mockRejectedValueOnce(new Error("download or signature check failed"));

      await expect(useUpdaterStore.getState().install("en")).resolves.toBeUndefined();

      const s = useUpdaterStore.getState();
      expect(s.phase).toBe("idle");
      expect(s.error).toBe("download or signature check failed");
    });

    it("clearOutcome removes the refusal and error", () => {
      useUpdaterStore.setState({ refusal: { code: "update_withdrawn" }, error: "boom" });

      useUpdaterStore.getState().clearOutcome();

      expect(useUpdaterStore.getState().refusal).toBeNull();
      expect(useUpdaterStore.getState().error).toBeNull();
    });
  });

  describe("reinstallPrevious", () => {
    it("reinstalls the previous version with progress", async () => {
      const ctl = controllableInstall();
      mockReinstall.mockImplementationOnce(ctl.impl);

      const pending = useUpdaterStore.getState().reinstallPrevious("fr", "1.1.0");
      expect(mockReinstall).toHaveBeenCalledWith("fr", expect.any(Function));
      ctl.progress({ event: "downloading", downloaded_bytes: 5, total_bytes: null });
      expect(useUpdaterStore.getState().phase).toBe("downloading");

      ctl.finish({ outcome: "restarting" });
      await pending;
      expect(useUpdaterStore.getState().phase).toBe("restarting");
      expect(useUpdaterStore.getState().installTargetVersion).toBe("1.1.0");
    });

    it("captures a failure", async () => {
      const ctl = controllableInstall();
      mockReinstall.mockImplementationOnce(ctl.impl);

      const pending = useUpdaterStore.getState().reinstallPrevious("en", "1.1.0");
      ctl.fail(new Error("no network"));
      await pending;

      expect(useUpdaterStore.getState()).toMatchObject({ phase: "idle", error: "no network" });
    });
  });

  describe("journal notice", () => {
    it("shows a rolled_back / install_not_completed notice until acknowledged", async () => {
      mockJournal.mockResolvedValueOnce(makeJournal());
      mockAcknowledge.mockResolvedValueOnce(undefined);

      await useUpdaterStore.getState().loadJournal();
      expect(pendingUpdateNotice(useUpdaterStore.getState())?.state).toBe("rolled_back");

      await useUpdaterStore.getState().acknowledgeNotice();
      expect(mockAcknowledge).toHaveBeenCalledTimes(1);
      expect(pendingUpdateNotice(useUpdaterStore.getState())).toBeNull();
    });

    it("shows no notice for other states or acknowledged ones", () => {
      expect(pendingUpdateNotice({ journal: makeJournal({ state: "completed" }) })).toBeNull();
      expect(
        pendingUpdateNotice({ journal: makeJournal({ notice_acknowledged: true }) }),
      ).toBeNull();
      expect(
        pendingUpdateNotice({ journal: makeJournal({ state: "install_not_completed" }) })?.state,
      ).toBe("install_not_completed");
    });

    it("an unreadable journal shows no notice", async () => {
      mockJournal.mockRejectedValueOnce(new Error("io"));

      await useUpdaterStore.getState().loadJournal();

      expect(useUpdaterStore.getState().journal).toBeNull();
    });
  });

  describe("install guard (step-up)", () => {
    it("runs the install through the guard and records the IPC code of a failure", async () => {
      mockInstall.mockRejectedValueOnce({
        code: "PERMISSION_DENIED",
        message: `Permission requise : ${P.ADM_SETTINGS}`,
      });
      useUpdaterStore.setState({ lastCheckResult: makeCheck() });
      let runs = 0;
      const run = <T>(action: () => Promise<T>): Promise<T> => {
        runs += 1;
        return action();
      };

      await useUpdaterStore.getState().install("en", { run, isCancelled: () => false });

      expect(runs).toBe(1);
      expect(useUpdaterStore.getState()).toMatchObject({
        phase: "idle",
        errorCode: "PERMISSION_DENIED",
      });
      expect(useUpdaterStore.getState().error).toContain(P.ADM_SETTINGS);
    });

    it("a cancelled identity check leaves no error", async () => {
      const cancelled = new Error("cancelled");
      useUpdaterStore.setState({ lastCheckResult: makeCheck() });

      await useUpdaterStore.getState().install("en", {
        run: () => Promise.reject(cancelled),
        isCancelled: (err) => err === cancelled,
      });

      expect(mockInstall).not.toHaveBeenCalled();
      expect(useUpdaterStore.getState()).toMatchObject({
        phase: "idle",
        error: null,
        errorCode: null,
      });
    });

    it("a mandatory update forced by the licence stays installable", async () => {
      mockInstall.mockResolvedValueOnce({ outcome: "restarting" });
      useUpdaterStore.setState({ lastCheckResult: makeCheck(), forceRequired: true });
      expect(isUpdateMandatory(useUpdaterStore.getState())).toBe(true);
      useUpdaterStore.getState().dismiss();
      expect(isOfferVisible(useUpdaterStore.getState())).toBe(true);

      await useUpdaterStore.getState().install("en");

      expect(mockInstall).toHaveBeenCalledTimes(1);
      expect(useUpdaterStore.getState().phase).toBe("restarting");
    });
  });
});

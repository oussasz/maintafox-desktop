// Tracks in-app update state across the application lifetime (master plan Phase 5.5).
// Architecture rule: only this store holds updater state; the update banner, the
// "check for updates" panel and the update recovery screen read from here instead
// of issuing fresh IPC calls.
//
// Pattern mirrors settings-store.ts: no write-through caching, actions call
// service functions and update local state, errors are captured not thrown.

import { create } from "zustand";

import { getProductLicenseOnboardingState } from "@/services/product-license-service";
import {
  type UpdateProgressHandler,
  acknowledgeUpdateNotice,
  checkForUpdate,
  getUpdateJournal,
  installPendingUpdate,
  reinstallPreviousVersion,
} from "@/services/updater-service";
import type { UpdateInstallPhase } from "@/types";
import { toErrorMessage } from "@/utils/errors";
import type {
  InstallOutcome,
  InstallRefusal,
  UpdateCheckResult,
  UpdateJournalView,
  UpdateProgressEvent,
} from "@shared/ipc-types";

/** Licence onboarding status meaning "this client must update before it may run". */
const FORCE_UPDATE_LICENSE_STATUS = "denied_force_update_required";

// ── State shape ───────────────────────────────────────────────────────────────

export interface UpdaterState {
  /** Last result from check_for_update — null before the first check answers. */
  lastCheckResult: UpdateCheckResult | null;
  /** True while a check IPC call is in flight. */
  isChecking: boolean;
  /** Non-null when the check itself failed at the IPC level (technical detail). */
  checkError: string | null;
  /** Activation policy says this client must update now (licence onboarding state). */
  forceRequired: boolean;
  /** Optional operator-facing rationale for the forced policy. */
  forceReason: string | null;
  /** Version whose offer the user dismissed (an optional update only). */
  dismissedVersion: string | null;

  /** Where the current install / reinstall is. */
  phase: UpdateInstallPhase;
  downloadedBytes: number;
  /** Total download size — null when the server does not announce it. */
  totalBytes: number | null;
  /** Version the current / last install puts in place (named in refusal messages). */
  installTargetVersion: string | null;
  /** Why the last install was not started (nothing was changed). */
  refusal: InstallRefusal | null;
  /** Non-null when the last install / reinstall failed (technical detail). */
  error: string | null;

  /** The last update's journal — null when never updated in the app or not loaded. */
  journal: UpdateJournalView | null;

  check: (locale?: string) => Promise<void>;
  install: (locale?: string) => Promise<void>;
  /** Recovery screen: reinstall `previousVersion` after a failed database update. */
  reinstallPrevious: (locale: string | undefined, previousVersion: string) => Promise<void>;
  loadJournal: () => Promise<void>;
  acknowledgeNotice: () => Promise<void>;
  /** Hides an optional update offer. Not allowed while the update is mandatory. */
  dismiss: () => void;
  /** Clears the last refusal / install error (the user has read it). */
  clearOutcome: () => void;
}

// ── Derived state ─────────────────────────────────────────────────────────────

/** The offered update must be installed: the manifest says so, or the licence policy does. */
export function isUpdateMandatory(
  s: Pick<UpdaterState, "lastCheckResult" | "forceRequired">,
): boolean {
  const offered = s.lastCheckResult?.available === true;
  return (offered && s.lastCheckResult?.mandatory === true) || s.forceRequired;
}

/** An update is offered and the user has not dismissed it (mandatory offers stay visible). */
export function isOfferVisible(
  s: Pick<UpdaterState, "lastCheckResult" | "forceRequired" | "dismissedVersion">,
): boolean {
  const result = s.lastCheckResult;
  if (!result?.available) return false;
  return isUpdateMandatory(s) || result.version !== s.dismissedVersion;
}

/** Outcome notice of the last update the user has not acknowledged yet, or null. */
export function pendingUpdateNotice(s: Pick<UpdaterState, "journal">): UpdateJournalView | null {
  const j = s.journal;
  if (!j || j.notice_acknowledged) return null;
  return j.state === "rolled_back" || j.state === "install_not_completed" ? j : null;
}

export function isInstallInProgress(phase: UpdateInstallPhase): boolean {
  return phase !== "idle";
}

function phaseFor(event: UpdateProgressEvent): UpdateInstallPhase {
  return event.event;
}

// ── Store ─────────────────────────────────────────────────────────────────────

export const useUpdaterStore = create<UpdaterState>()((set, get) => {
  const onProgress: UpdateProgressHandler = (event) => {
    if (event.event === "downloading") {
      set({
        phase: "downloading",
        downloadedBytes: event.downloaded_bytes,
        totalBytes: event.total_bytes,
      });
    } else {
      set({ phase: phaseFor(event) });
    }
  };

  /** Runs one install-like command and records its outcome. */
  const runInstall = async (
    command: () => Promise<InstallOutcome>,
    locale: string | undefined,
    targetVersion: string | null,
  ): Promise<void> => {
    if (isInstallInProgress(get().phase)) return;
    set({
      installTargetVersion: targetVersion,
      phase: "backing_up",
      downloadedBytes: 0,
      totalBytes: null,
      refusal: null,
      error: null,
    });
    try {
      const outcome = await command();
      if (outcome.outcome === "restarting") {
        // The installer runs and this process exits: keep showing "restarting".
        set({ phase: "restarting" });
        return;
      }
      set({ phase: "idle", refusal: outcome.refusal });
      if (outcome.refusal.code === "update_withdrawn") {
        // The offer changed since the last check: refresh it so the banner is truthful.
        await get().check(locale);
      }
    } catch (err) {
      set({ phase: "idle", error: toErrorMessage(err) });
    }
  };

  return {
    lastCheckResult: null,
    isChecking: false,
    checkError: null,
    forceRequired: false,
    forceReason: null,
    dismissedVersion: null,
    phase: "idle",
    downloadedBytes: 0,
    totalBytes: null,
    installTargetVersion: null,
    refusal: null,
    error: null,
    journal: null,

    check: async (locale) => {
      set({ isChecking: true, checkError: null });
      try {
        const [result, onboarding] = await Promise.all([
          checkForUpdate(locale),
          getProductLicenseOnboardingState().catch((err: unknown) => {
            // The licence lookup only adds the forced-update policy; the manifest's
            // `mandatory` flag stays the primary signal.
            console.warn("updater-store: licence state unavailable (non-fatal)", err);
            return null;
          }),
        ]);
        const forceRequired = onboarding?.status === FORCE_UPDATE_LICENSE_STATUS;
        set({
          lastCheckResult: result,
          isChecking: false,
          forceRequired,
          forceReason: forceRequired ? (onboarding?.deny_message ?? null) : null,
        });
      } catch (err) {
        // Non-fatal — an update check failure must never block the application.
        // The Rust command reports network problems in the result (status "unreachable"),
        // so this path only fires on IPC bridge / contract failures.
        console.warn("updater-store: check failed (non-fatal)", err);
        set({ isChecking: false, checkError: toErrorMessage(err) });
      }
    },

    install: (locale) =>
      runInstall(
        () => installPendingUpdate(locale, onProgress),
        locale,
        get().lastCheckResult?.version ?? null,
      ),

    reinstallPrevious: (locale, previousVersion) =>
      runInstall(() => reinstallPreviousVersion(locale, onProgress), locale, previousVersion),

    loadJournal: async () => {
      try {
        set({ journal: await getUpdateJournal() });
      } catch (err) {
        // The notice is informative: without the journal no notice is shown.
        console.warn("updater-store: update journal unavailable (non-fatal)", err);
      }
    },

    acknowledgeNotice: async () => {
      const journal = get().journal;
      if (!journal) return;
      // Hidden right away; if recording fails the notice is shown again at the next start.
      set({ journal: { ...journal, notice_acknowledged: true } });
      try {
        await acknowledgeUpdateNotice();
      } catch (err) {
        console.warn("updater-store: update notice acknowledgement not recorded", err);
      }
    },

    dismiss: () => {
      const s = get();
      if (isUpdateMandatory(s) || isInstallInProgress(s.phase)) return;
      set({
        dismissedVersion: s.lastCheckResult?.version ?? s.dismissedVersion,
        refusal: null,
        error: null,
      });
    },

    clearOutcome: () => set({ refusal: null, error: null }),
  };
});

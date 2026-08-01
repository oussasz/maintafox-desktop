// Tracks update-check state across the application lifetime.
// Architecture rule: only this store holds updater state; the notification
// banner and settings page read from here instead of issuing fresh IPC calls.
//
// Pattern mirrors settings-store.ts: no write-through caching, actions call
// service functions and update local state, errors are captured not thrown.

import { create } from "zustand";

import { getProductLicenseOnboardingState } from "@/services/product-license-service";
import { checkForUpdate, installPendingUpdate } from "@/services/updater-service";
import { toErrorMessage } from "@/utils/errors";
import type { UpdateCheckResult } from "@shared/ipc-types";

// ── State shape ───────────────────────────────────────────────────────────────

interface UpdaterState {
  /** Last result from check_for_update — null before the first check fires. */
  lastCheckResult: UpdateCheckResult | null;
  /** True while a check IPC call is in flight. */
  isChecking: boolean;
  /** True while install_pending_update is downloading/applying. */
  isInstalling: boolean;
  /** True after install completes (application will restart shortly). */
  installComplete: boolean;
  /** Activation policy says this client must update now. */
  forceRequired: boolean;
  /** Optional operator-facing rationale for forced policy. */
  forceReason: string | null;
  /** Non-null if the last check or install operation encountered an error. */
  error: string | null;

  checkForUpdate: () => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissNotification: () => void;
}

// ── Store ─────────────────────────────────────────────────────────────────────

export const useUpdaterStore = create<UpdaterState>()((set) => ({
  lastCheckResult: null,
  isChecking: false,
  isInstalling: false,
  installComplete: false,
  forceRequired: false,
  forceReason: null,
  error: null,

  checkForUpdate: async () => {
    set({ isChecking: true, error: null });
    try {
      const [result, onboarding] = await Promise.all([
        checkForUpdate(),
        getProductLicenseOnboardingState().catch(() => null),
      ]);
      const forceRequired = onboarding?.status === "denied_force_update_required";
      const forceReason = onboarding?.deny_message ?? null;
      set({
        lastCheckResult: result,
        isChecking: false,
        forceRequired,
        forceReason,
        error:
          forceRequired && !result.available
            ? forceReason ?? "A forced update policy is active but no update package is currently available."
            : null,
      });
    } catch (err) {
      // Non-fatal — update check failure must never block the application.
      // The Rust layer already swallows manifest network errors and returns
      // available:false, so this path only fires on IPC bridge failures (dev/test).
      console.warn("updater-store: check failed (non-fatal)", err);
      set({
        isChecking: false,
        forceRequired: false,
        forceReason: null,
        error: toErrorMessage(err),
      });
    }
  },

  installUpdate: async () => {
    set({ isInstalling: true, error: null });
    try {
      await installPendingUpdate();
      // After a successful install the Rust side calls app.restart(); the app
      // process will exit. Setting installComplete:true gives the UI a brief
      // window to show "Restarting…" before the process terminates.
      set({ isInstalling: false, installComplete: true });
    } catch (err) {
      set({
        isInstalling: false,
        error: toErrorMessage(err),
      });
    }
  },

  dismissNotification: () =>
    set({ lastCheckResult: null, error: null, installComplete: false, forceRequired: false, forceReason: null }),
}));

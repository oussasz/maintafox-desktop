// useUpdater — React hooks for in-app updates (master plan Phase 5.5).
//
// useUpdateScheduler(): performs an initial check (and loads the last update's journal) on
//   mount, then rechecks at CHECK_INTERVAL_MS. Mount ONCE in the application shell — multiple
//   mounts would trigger duplicate intervals.
// useUpdater(): state + actions for update UI (banner, "check for updates" panel). No effects:
//   safe to use in several components.
//
// check_for_update does NOT require a session. install_pending_update DOES require one — the
// Rust command enforces this; the UI presents a confirmation dialog before calling install().
// The current UI locale is passed to the commands so server-side texts match the UI language.

import { useCallback, useEffect } from "react";

import { useLocaleStore } from "@/stores/locale-store";
import {
  isOfferVisible,
  isUpdateMandatory,
  pendingUpdateNotice,
  useUpdaterStore,
} from "@/stores/updater-store";
import type { UseUpdaterResult } from "@/types";

// Re-check every 2 hours while the application is running.
export const CHECK_INTERVAL_MS = 2 * 60 * 60 * 1000;

function currentLocale(): string {
  return useLocaleStore.getState().activeLocale;
}

/** `enabled`: start checking only once the backend has finished starting up. */
export function useUpdateScheduler(enabled: boolean): void {
  const check = useUpdaterStore((s) => s.check);
  const loadJournal = useUpdaterStore((s) => s.loadJournal);

  // check / loadJournal are stable (defined inside zustand create()), so the effect does not
  // re-register on every render.
  useEffect(() => {
    if (!enabled) return;
    void loadJournal();
    void check(currentLocale());
    const id = setInterval(() => void check(currentLocale()), CHECK_INTERVAL_MS);
    return () => clearInterval(id);
  }, [enabled, check, loadJournal]);
}

export function useUpdater(): UseUpdaterResult {
  const s = useUpdaterStore();

  const { check, install: installAction, dismiss, acknowledgeNotice, clearOutcome } = s;
  const checkNow = useCallback(() => void check(currentLocale()), [check]);
  const install = useCallback(() => void installAction(currentLocale()), [installAction]);
  const acknowledge = useCallback(() => void acknowledgeNotice(), [acknowledgeNotice]);

  return {
    result: s.lastCheckResult,
    showOffer: isOfferVisible(s),
    mandatory: isUpdateMandatory(s),
    forceRequired: s.forceRequired,
    forceReason: s.forceReason,
    isChecking: s.isChecking,
    checkError: s.checkError,
    phase: s.phase,
    downloadedBytes: s.downloadedBytes,
    totalBytes: s.totalBytes,
    installTargetVersion: s.installTargetVersion,
    refusal: s.refusal,
    error: s.error,
    notice: pendingUpdateNotice(s),
    checkNow,
    install,
    dismiss,
    acknowledgeNotice: acknowledge,
    clearOutcome,
  };
}

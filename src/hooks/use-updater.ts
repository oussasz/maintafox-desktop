// useUpdater — React hook for the in-app update notification system.
//
// Performs an initial check on mount, then rechecks at CHECK_INTERVAL_MS.
// check_for_update does NOT require a session, so this hook is safe to
// mount on the login screen or the main shell without guarding on auth.
// install_pending_update DOES require a session — the Rust command enforces
// this; the UI layer should present a confirmation dialog before calling install().
//
// Usage:
//   const { available, version, notes, isInstalling, install, dismiss }
//     = useUpdater();
//
// Mount once in the application shell (e.g., App.tsx or MainLayout).
// Multiple mounts will trigger duplicate intervals — avoid mounting in leaves.

import { useEffect, useCallback } from "react";

import { useUpdaterStore } from "@/stores/updater-store";
import type { UseUpdaterResult } from "@/types";

// Re-check every 2 hours while the application is running.
const CHECK_INTERVAL_MS = 2 * 60 * 60 * 1000;

export function useUpdater(): UseUpdaterResult {
  const {
    lastCheckResult,
    isChecking,
    isInstalling,
    installComplete,
    forceRequired,
    forceReason,
    error,
    checkForUpdate,
    installUpdate,
    dismissNotification,
  } = useUpdaterStore();

  // Initial check on mount then periodic interval.
  // checkForUpdate is stable across renders (defined inside zustand create())
  // so the effect's dependency is safe and does not re-register on every render.
  useEffect(() => {
    void checkForUpdate();
    const id = setInterval(() => void checkForUpdate(), CHECK_INTERVAL_MS);
    return () => clearInterval(id);
  }, [checkForUpdate]);

  const checkNow = useCallback(() => void checkForUpdate(), [checkForUpdate]);
  const install = useCallback(() => void installUpdate(), [installUpdate]);
  const dismiss = useCallback(() => dismissNotification(), [dismissNotification]);

  return {
    available: lastCheckResult?.available ?? false,
    version: lastCheckResult?.version ?? null,
    notes: lastCheckResult?.notes ?? null,
    forceRequired,
    forceReason,
    isChecking,
    isInstalling,
    installComplete,
    error,
    checkNow,
    install,
    dismiss,
  };
}

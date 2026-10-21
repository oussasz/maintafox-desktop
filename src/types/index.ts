import type { InstallRefusal, UpdateCheckResult, UpdateJournalView } from "@shared/ipc-types";

/** Application-level status values used in the UI */
export type AppStatus =
  | "loading"
  | "ready"
  | "error"
  | "offline"
  | "database_key_required"
  /** The installed update could not update the database: reinstall the previous version. */
  | "update_rollback_required";

/** Semantic status badge variants */
export type StatusVariant = "success" | "danger" | "warning" | "info" | "neutral";

/** Module identifier keys — one per PRD §6 module */
export type ModuleKey =
  | "dashboard"
  | "auth"
  | "org"
  | "equipment"
  | "requests"
  | "work-orders"
  | "personnel"
  | "users"
  | "inventory"
  | "pm"
  | "reliability"
  | "analytics"
  | "archive"
  | "lookups"
  | "notifications"
  | "documentation"
  | "planning"
  | "activity"
  | "settings"
  | "profile"
  | "training"
  | "permits"
  | "budget"
  | "inspections";

export type Theme = "dark" | "light";

// ─── Updater hook ─────────────────────────────────────────────────────────────

/** Where an in-app install / reinstall is (driven by the Rust progress channel). */
export type UpdateInstallPhase =
  | "idle"
  | "starting"
  | "backing_up"
  | "restoring_backup"
  | "downloading"
  | "installing"
  | "restarting";

/**
 * How the UI runs the install command: the step-up dialog (`useStepUp().withStepUp`) asks for a
 * fresh identity check when the command answers STEP_UP_REQUIRED, then retries. `isCancelled`
 * recognises the user closing that dialog (nothing happened: no error is shown).
 */
export interface InstallGuard {
  run: <T>(action: () => Promise<T>) => Promise<T>;
  isCancelled: (err: unknown) => boolean;
}

export interface UseUpdaterResult {
  /** Last check result — null before the first check answers. */
  result: UpdateCheckResult | null;
  /** True when an update is offered and not dismissed (always true while mandatory). */
  showOffer: boolean;
  /** The offered update must be installed: manifest `mandatory`, or the licence policy. */
  mandatory: boolean;
  /** Licence policy requires an update (product licence onboarding state). */
  forceRequired: boolean;
  forceReason: string | null;
  isChecking: boolean;
  /** Check failed at the IPC level (technical detail). */
  checkError: string | null;
  phase: UpdateInstallPhase;
  downloadedBytes: number;
  totalBytes: number | null;
  /** Version the current / last install puts in place (named in refusal messages). */
  installTargetVersion: string | null;
  refusal: InstallRefusal | null;
  /** Install failed (technical detail; the UI adds a plain-language explanation). */
  error: string | null;
  /** IPC code of that failure (`PERMISSION_DENIED`, `AUTH_ERROR`, …) or null. */
  errorCode: string | null;
  /** Outcome notice of the last update to show once (rolled back / not completed), or null. */
  notice: UpdateJournalView | null;
  checkNow: () => void;
  /** Installs the offered update; `guard` runs it through the step-up dialog. */
  install: (guard?: InstallGuard) => void;
  dismiss: () => void;
  acknowledgeNotice: () => void;
  clearOutcome: () => void;
}

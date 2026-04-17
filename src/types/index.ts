/** Application-level status values used in the UI */
export type AppStatus = "loading" | "ready" | "error" | "offline";

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
  | "iot"
  | "erp"
  | "permits"
  | "budget"
  | "inspections"
  | "configuration";

export type Theme = "dark" | "light";

// ─── Updater hook ─────────────────────────────────────────────────────────────

export interface UseUpdaterResult {
  available: boolean;
  version: string | null;
  notes: string | null;
  forceRequired: boolean;
  forceReason: string | null;
  isChecking: boolean;
  isInstalling: boolean;
  installComplete: boolean;
  error: string | null;
  checkNow: () => void;
  install: () => void;
  dismiss: () => void;
}

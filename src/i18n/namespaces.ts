// src/i18n/namespaces.ts
// Canonical registry of all i18n namespaces.
// Every namespace here must have both a fr/{ns}.json and en/{ns}.json file.
// This file is the single source of truth — no namespace string should be
// written manually anywhere else in the codebase.

/**
 * Global namespaces: loaded eagerly at startup because they are used
 * on every screen (error messages, validation, date/number formats, etc.)
 */
export const EAGER_NAMESPACES = [
  "common", // App-wide labels: app name, loading, actions, confirmations
  "auth", // Login screen, session expiry, force-password-change
  "errors", // Error messages for all AppError variants
  "validation", // Form validation messages (required, min, max, pattern, etc.)
  "formats", // Date, number, currency format tokens (not UI strings)
  "shell", // Top bar, sidebar, status bar, notifications badge
] as const;

/**
 * Module namespaces: loaded lazily when the module route is first visited.
 * The key is the route slug / module code; the value is the namespace name.
 */
export const MODULE_NAMESPACES = {
  equipment: "equipment", // §6.3 Equipment
  di: "di", // §6.4 Intervention Requests
  ot: "ot", // §6.5 Work Orders
  org: "org", // §6.2 Organization
  personnel: "personnel", // §6.6 Personnel
  reference: "reference", // §6.13 Reference Data / Lookups
  inventory: "inventory", // §6.8 Inventory
  pm: "pm", // §6.9 Preventive Maintenance
  planning: "planning", // §6.16 Planning
  permits: "permits", // §6.23 Work Permits
  inspections: "inspections", // §6.25 Inspection Rounds
  training: "training", // §6.20 Training & Habilitation
  reliability: "reliability", // §6.11 RAMS / Reliability
  budget: "budget", // §6.24 Budget & Cost Centers
  reports: "reports", // §6.12 Reports & Analytics
  archive: "archive", // §6.12 Archive Explorer
  notifications: "notifications", // §6.14 Notification System
  documentation: "documentation", // §6.15 Documentation & Support Center
  iot: "iot", // §6.21 IoT Gateway
  erp: "erp", // §6.22 ERP Connector
  activity: "activity", // §6.17 Activity Feed & Audit Log
  users: "users", // §6.7 Users & Roles Admin UI
  admin: "admin", // §6.7 Administration panels, metrics, and role management
  settings: "settings", // §6.18 Application Settings
  diagnostics: "diagnostics", // §6.20 Diagnostics & Support Bundle
  configuration: "configuration", // §6.26 Configuration Engine
} as const;

export type EagerNamespace = (typeof EAGER_NAMESPACES)[number];
export type ModuleNamespace = (typeof MODULE_NAMESPACES)[keyof typeof MODULE_NAMESPACES];
export type AppNamespace = EagerNamespace | ModuleNamespace;

/** All namespace names in one flat array (for type generation and tests) */
export const ALL_NAMESPACES: AppNamespace[] = [
  ...EAGER_NAMESPACES,
  ...Object.values(MODULE_NAMESPACES),
];

/** Supported locale codes */
export const SUPPORTED_LOCALES = ["fr", "en"] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

/** Display names for each locale (used in the language selector UI) */
export const LOCALE_DISPLAY_NAMES: Record<SupportedLocale, string> = {
  fr: "Français",
  en: "English",
};

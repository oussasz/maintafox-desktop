/**
 * Registry of the in-app module guides. Texts live in the `help` i18n namespace
 * (`modules.<id>.title|purpose|step1..stepN`).
 *
 * Visibility is NOT redefined here: a guide points at an entry of the navigation registry
 * (`navKey`), so it is listed exactly when the sidebar lists that module (same right, same
 * alternative rights, same licence check). A guide without `navKey` is open to every signed-in user.
 */
export interface HelpGuide {
  id: string;
  /** Key of the entry in `defaultNavItems` that decides who sees the module. */
  navKey?: string;
  /** Where "Ouvrir le module" goes; defaults to the nav entry path. */
  path?: string;
  /** Number of `stepN` texts in the locale file. */
  steps: number;
}

export const HELP_GUIDES: HelpGuide[] = [
  { id: "dashboard", navKey: "dashboard", steps: 3 },
  { id: "requests", navKey: "requests", steps: 4 },
  { id: "workOrders", navKey: "work-orders", steps: 4 },
  { id: "planning", navKey: "planning", steps: 3 },
  { id: "pm", navKey: "pm", steps: 4 },
  { id: "equipment", navKey: "equipment", steps: 4 },
  { id: "inventory", navKey: "inventory", steps: 4 },
  { id: "permits", navKey: "permits", steps: 3 },
  { id: "inspections", navKey: "inspections", steps: 4 },
  { id: "personnel", navKey: "personnel", steps: 3 },
  { id: "reliability", navKey: "reliability", steps: 3 },
  { id: "analytics", navKey: "analytics", steps: 3 },
  { id: "admin", navKey: "admin", steps: 3 },
  { id: "lookups", navKey: "lookups", steps: 3 },
  // Sync status lives in Settings, which every signed-in user can open.
  { id: "sync", navKey: "settings", path: "/settings?tab=integrations", steps: 3 },
  { id: "documentation", navKey: "documentation", steps: 4 },
];

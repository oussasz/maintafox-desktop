// src/i18n/types.ts
// TypeScript module augmentation for i18next type-safe keys.
//
// HOW IT WORKS:
// i18next checks for the `CustomTypeOptions` interface augmentation to narrow
// the return type of t() calls. If the namespace/key path is not found in the
// imported JSON types, TypeScript emits a type error.
//
// SCOPE: Only the EAGER namespaces are typed here. Module namespaces use
// lazy loading and their JSON is not reliably available at compile time.
// For module namespaces, use t("key") without type narrowing.
//
// This can be extended per-module in Phase 2 by augmenting this interface
// with module-specific JSON imports.

import type frAuth from "./fr/auth.json";
import type frCommon from "./fr/common.json";
import type frErrors from "./fr/errors.json";
import type frShell from "./fr/shell.json";
import type frValidation from "./fr/validation.json";
// Module namespaces with compile-time type definitions:
import type frDashboard from "./locale-data/fr/dashboard.json";
import type frDi from "./locale-data/fr/di.json";
import type frDiagnostics from "./locale-data/fr/diagnostics.json";
import type frEquipment from "./locale-data/fr/equipment.json";
import type frOrg from "./locale-data/fr/org.json";
import type frReference from "./locale-data/fr/reference.json";
import type frSettings from "./locale-data/fr/settings.json";

declare module "i18next" {
  interface CustomTypeOptions {
    // The default namespace (used when no ns is passed to useTranslation)
    defaultNS: "common";
    // Type-check all keys in these namespaces:
    resources: {
      common: typeof frCommon;
      auth: typeof frAuth;
      errors: typeof frErrors;
      validation: typeof frValidation;
      shell: typeof frShell;
      // Module namespaces (lazy-loaded at runtime, typed at compile time):
      dashboard: typeof frDashboard;
      di: typeof frDi;
      diagnostics: typeof frDiagnostics;
      equipment: typeof frEquipment;
      org: typeof frOrg;
      reference: typeof frReference;
      settings: typeof frSettings;
      // Allow any additional lazy namespace to compile without hard-failing
      // on key narrowing when module JSON evolves independently.
      [namespace: string]: Record<string, unknown>;
    };
  }
}

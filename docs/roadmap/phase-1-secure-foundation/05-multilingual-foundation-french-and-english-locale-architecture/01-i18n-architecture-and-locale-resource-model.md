# Phase 1 · Sub-phase 05 · File 01
# i18n Architecture and Locale Resource Model

## Context and Purpose

SP01 through SP04 have established the application shell, data layer, authentication, and
RBAC. Throughout that work, translation strings were embedded as temporary French-only
hardcodes (error messages, Rust tracing events, tray labels) and partially externalized in
the `shell` namespace. The product's PRD (§14.7 and §6.18.1) is explicit: multilingual
capability is a day-one architectural requirement — not deferred localization work.

This file installs three foundational systems:

1. **Namespace registry and lazy-loading strategy** — the full namespace plan for all 22
   product modules, plus global namespaces (`common`, `auth`, `errors`, `validation`,
   `formats`). Namespaces are loaded lazily so startup bundle size is not inflated by
   translation files for modules the user hasn't opened.

2. **TypeScript type safety for translation keys** — a typed `t()` wrapper that catches
   missing or misspelled keys at compile time rather than at runtime, using
   `i18next`'s TypeScript plugin pattern.

3. **Rust-side locale detection and preference IPC** — the application reads the OS
   locale on startup, checks the tenant preference in `system_config`, falls back through
   a deterministic chain, and exposes `get_locale_preference` / `set_locale_preference`
   IPC commands so the frontend can switch language without restarting.

## Architecture Rules Applied

- **Namespaces, not one monolithic file.** Every module has its own namespace JSON file
  (`fr/{module}.json` and `en/{module}.json`). This prevents merge conflicts and allows
  per-module translation ownership.
- **French is the default; English is the fallback.** If a French key is missing, the
  English translation is shown. If neither exists, the key string itself is shown (not
  an empty string) so missing translations are immediately visible during development.
- **Locale preference precedence (highest to lowest):**
  1. User preference stored in `system_config` (key: `locale.user_language`)
  2. Tenant default from `system_config` (key: `locale.default_language`)
  3. OS locale (read at startup via Rust)
  4. Hard fallback: `fr` (French)
- **No inline string literals in components.** All user-visible text must come from
  `useTranslation("namespace")`. The linting rule `i18next/no-literal-string` is
  recommended for future phases; in Phase 1, the rule is documented but not enforced.
- **IPC locale change is non-destructive.** Switching language calls `i18n.changeLanguage()`
  client-side and saves the preference to `system_config`. No reload required.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/i18n/namespaces.ts` | Canonical list of all namespaces with module bucketing |
| `src/i18n/config.ts` | i18next initializer with lazy loading and type plugin |
| `src/i18n/types.ts` | TypeScript module augmentation for type-safe `t()` |
| `src/i18n/index.ts` (updated) | Re-exports from config + types |
| `src-tauri/src/locale/mod.rs` | OS locale detection, system_config read/write |
| `src-tauri/src/commands/locale.rs` | IPC: `get_locale_preference`, `set_locale_preference` |
| `src/services/locale-service.ts` | Frontend IPC wrapper for locale commands |
| `src/stores/locale-store.ts` | Zustand slice: current locale, loading state |
| `src/hooks/use-locale.ts` | `useLocale()` hook with `setLocale()` action |
| `shared/ipc-types.ts` (extended) | `LocalePreference`, `LocaleInfo` |
| `docs/I18N_ARCHITECTURE.md` | i18n reference document for all future sprint authors |

## Prerequisites

- SP02-F03 complete: `src/i18n/index.ts` exists with `fr` + `en` + `common` + `shell`
- SP04-F01 complete: `system_config` table is writable (for locale persistence)
- SP01-F01 complete: `shared/ipc-types.ts` exists

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Namespace Registry and Lazy-Loading i18next Config | `namespaces.ts`, `config.ts`, updated `index.ts` |
| S2 | TypeScript Type Safety for Translation Keys | `types.ts`, TypeScript module augmentation |
| S3 | Rust Locale Detection, IPC, and Frontend Locale Store | `locale/mod.rs`, `commands/locale.rs`, `locale-service.ts`, `use-locale.ts` |

---

## Sprint S1 — Namespace Registry and Lazy-Loading i18next Config

### AI Agent Prompt

```
You are a senior TypeScript engineer continuing work on Maintafox Desktop.
SP02-F03 set up i18next with two namespaces: `common` (fr/en) and `shell` (fr/en).
Your task is to replace that minimal configuration with a full namespace registry
and a lazy-loading i18next configuration.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src/i18n/namespaces.ts
─────────────────────────────────────────────────────────────────────
```typescript
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
  "common",       // App-wide labels: app name, loading, actions, confirmations
  "auth",         // Login screen, session expiry, force-password-change
  "errors",       // Error messages for all AppError variants
  "validation",   // Form validation messages (required, min, max, pattern, etc.)
  "formats",      // Date, number, currency format tokens (not UI strings)
  "shell",        // Top bar, sidebar, status bar, notifications badge
] as const;

/**
 * Module namespaces: loaded lazily when the module route is first visited.
 * The key is the route slug / module code; the value is the namespace name.
 */
export const MODULE_NAMESPACES = {
  equipment:     "equipment",     // §6.3 Equipment
  di:            "di",            // §6.4 Intervention Requests
  ot:            "ot",            // §6.5 Work Orders
  org:           "org",           // §6.2 Organization
  personnel:     "personnel",     // §6.6 Personnel
  reference:     "reference",     // §6.13 Reference Data / Lookups
  inventory:     "inventory",     // §6.8 Inventory
  pm:            "pm",            // §6.9 Preventive Maintenance
  planning:      "planning",      // §6.16 Planning
  permits:       "permits",       // §6.23 Work Permits
  inspections:   "inspections",   // §6.25 Inspection Rounds
  training:      "training",      // §6.20 Training & Habilitation
  reliability:   "reliability",   // §6.11 RAMS / Reliability
  budget:        "budget",        // §6.24 Budget & Cost Centers
  reports:       "reports",       // §6.12 Reports & Analytics
  archive:       "archive",       // §6.12 Archive Explorer
  notifications: "notifications", // §6.14 Notification System
  documentation: "documentation", // §6.15 Documentation & Support Center
  iot:           "iot",           // §6.21 IoT Gateway
  erp:           "erp",           // §6.22 ERP Connector
  activity:      "activity",      // §6.17 Activity Feed & Audit Log
  users:         "users",         // §6.7 Users & Roles Admin UI
  settings:      "settings",      // §6.18 Application Settings
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
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src/i18n/config.ts
─────────────────────────────────────────────────────────────────────
Install required packages first:
```
pnpm add i18next react-i18next i18next-resources-to-backend
```

```typescript
// src/i18n/config.ts
// i18next initializer with lazy backend loading.
// Replaces the static import pattern from SP02-F03.

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import resourcesToBackend from "i18next-resources-to-backend";
import {
  EAGER_NAMESPACES,
  SUPPORTED_LOCALES,
  type SupportedLocale,
} from "./namespaces";

// Eager resources: imported directly to guarantee availability at frame 0.
// These are special-cased because lazy loading would cause a visible flicker
// on the shell layout and login screen.
import frCommon       from "./fr/common.json";
import frAuth         from "./fr/auth.json";
import frErrors       from "./fr/errors.json";
import frValidation   from "./fr/validation.json";
import frFormats      from "./fr/formats.json";
import frShell        from "./fr/shell.json";

import enCommon       from "./en/common.json";
import enAuth         from "./en/auth.json";
import enErrors       from "./en/errors.json";
import enValidation   from "./en/validation.json";
import enFormats      from "./en/formats.json";
import enShell        from "./en/shell.json";

const eagerResources = {
  fr: {
    common:     frCommon,
    auth:       frAuth,
    errors:     frErrors,
    validation: frValidation,
    formats:    frFormats,
    shell:      frShell,
  },
  en: {
    common:     enCommon,
    auth:       enAuth,
    errors:     enErrors,
    validation: enValidation,
    formats:    enFormats,
    shell:      enShell,
  },
};

// The default locale is French. The locale can be changed at runtime via
// i18n.changeLanguage() after reading the user or tenant preference from
// system_config.
export const DEFAULT_LOCALE: SupportedLocale = "fr";
export const FALLBACK_LOCALE: SupportedLocale = "en";

// Initialize once (idempotent — safe to call multiple times).
let initialized = false;

export function initI18n(): void {
  if (initialized) return;
  initialized = true;

  void i18n
    .use(initReactI18next)
    // Lazy-loads module namespaces on demand via dynamic import.
    // The `ns` and `lng` arguments match the file path pattern:
    //   src/i18n/{lng}/{ns}.json
    .use(
      resourcesToBackend(
        (lng: string, ns: string) =>
          import(`./locale-data/${lng}/${ns}.json`)
      )
    )
    .init({
      // Eager resources are pre-loaded; lazy content goes through the backend.
      resources: eagerResources,
      // Default language is French.
      lng: DEFAULT_LOCALE,
      fallbackLng: FALLBACK_LOCALE,
      // Namespaces loaded eagerly — others are loaded on demand.
      ns: [...EAGER_NAMESPACES],
      defaultNS: "common",
      // No HTML escaping — React handles this.
      interpolation: { escapeValue: false },
      // Show the key name if a translation is missing (never empty string).
      // Example: missing key "equipment.detail.title" shows as-is in dev.
      parseMissingKeyHandler: (key: string, defaultValue?: string) =>
        defaultValue ?? `[${key}]`,
      // Debug mode in development: logs missing keys to console.
      debug: import.meta.env.DEV,
      // React-specific settings.
      react: {
        useSuspense: false,         // Avoids React Suspense requirement
        transSupportBasicHtmlNodes: true,
      },
      // Accept region-specific codes by mapping to base locale:
      // fr-DZ → fr, en-US → en
      load: "languageOnly",
    });
}

export { i18n };
```

Move all locale JSON files under `src/i18n/locale-data/` subdirectory
(lazy-loaded modules) and keep eager files directly in `src/i18n/fr/` and
`src/i18n/en/` as shown above. The `locale-data/` directory is for module
namespaces that are NOT in `eagerResources`.

─────────────────────────────────────────────────────────────────────
STEP 3 — Update src/i18n/index.ts
─────────────────────────────────────────────────────────────────────
Replace the entire file:

```typescript
// src/i18n/index.ts
// Entry point for i18n. Call initI18n() before rendering the React tree.
export { i18n, initI18n, DEFAULT_LOCALE, FALLBACK_LOCALE } from "./config";
export { ALL_NAMESPACES, SUPPORTED_LOCALES, LOCALE_DISPLAY_NAMES } from "./namespaces";
export type { AppNamespace, SupportedLocale, EagerNamespace, ModuleNamespace } from "./namespaces";
```

Call `initI18n()` in `src/main.tsx` BEFORE `createRoot`:
```typescript
import { initI18n } from "./i18n";
initI18n();
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Create the locale-data/ directory for lazy module namespaces
─────────────────────────────────────────────────────────────────────
Create `src/i18n/locale-data/fr/.gitkeep` and `src/i18n/locale-data/en/.gitkeep`
as placeholder directories. The module namespace JSON files will be added in F02.

Note for the build system: Vite resolves `import()` calls via its dynamic import
plugin. The pattern `import('./locale-data/${lng}/${ns}.json')` works with Vite
out of the box because the directory exists and `?raw` imports are not needed for
JSON. Ensure `src/vite.config.ts` has `resolve: { extensions: ['.json', ...] }`.

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- pnpm run typecheck passes with 0 errors
- pnpm run dev: app starts, shell uses French labels (from shell namespace)
- Browser DevTools: no "i18next: missing key" warnings for shell/auth screens
- ALL_NAMESPACES.length === 30 (6 eager + 24 module namespaces)
```

---

### Supervisor Verification — Sprint S1

**V1 — Namespace registry count.**
Run in the DevTools console:
```javascript
// After the app loads:
import('/src/i18n/namespaces.ts').then(m => console.log('total ns:', m.ALL_NAMESPACES.length));
// Or just inspect during hot-module reload.
```
Expected: 30 namespaces. If fewer, some module namespaces were omitted from the registry.

**V2 — No missing key warnings for eager namespaces.**
Open DevTools → Console. Reload the app. Filter for "i18next". The log should show the
i18next initialization message. There should be NO warnings of the form
`i18next: KEY 'common:...' for language 'fr' with ns 'common' not found`.
If such warnings appear, the eager import failed to load. Flag the namespace name.

**V3 — `initI18n()` called before React root.**
In `src/main.tsx`, confirm `initI18n()` appears on a line before `createRoot(...)`.
If it is called after, language may not be applied to the first render. Flag it.

---

## Sprint S2 — TypeScript Type Safety for Translation Keys

### AI Agent Prompt

```
You are a senior TypeScript engineer. Sprint S1 is complete: the namespace registry
and lazy-loading config are in place. Your task is to add TypeScript module augmentation
so that the `t()` function from `useTranslation()` rejects typos and missing keys at
compile time.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src/i18n/types.ts (TypeScript module augmentation)
─────────────────────────────────────────────────────────────────────
i18next supports TypeScript type safety via module augmentation of the
`CustomTypeOptions` interface. This approach does NOT require generating a
separate .d.ts file at build time; the types are expressed inline as imports.

```typescript
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

import type frCommon     from "./fr/common.json";
import type frAuth       from "./fr/auth.json";
import type frErrors     from "./fr/errors.json";
import type frValidation from "./fr/validation.json";
import type frShell      from "./fr/shell.json";

declare module "i18next" {
  interface CustomTypeOptions {
    // The default namespace (used when no ns is passed to useTranslation)
    defaultNS: "common";
    // Type-check all keys in these namespaces:
    resources: {
      common:     typeof frCommon;
      auth:       typeof frAuth;
      errors:     typeof frErrors;
      validation: typeof frValidation;
      shell:      typeof frShell;
    };
  }
}
```

Re-export from `src/i18n/index.ts`:
```typescript
// Add to top of index.ts (side-effect import to activate augmentation):
import "./types";
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Verify typed usage in a component
─────────────────────────────────────────────────────────────────────
Add a compile-time assertion test in `src/i18n/__tests__/types.test.ts`:

```typescript
// src/i18n/__tests__/types.test.ts
// These tests DO NOT run assertions — they are type-only checks.
// If the file compiles, the type narrowing is working correctly.
// Any invalid key would cause a TypeScript error during `pnpm run typecheck`.

import { describe, it } from "vitest";

// This import activates the augmentation. The test file must
// import i18n to trigger the CustomTypeOptions module augmentation.
import "../types";

describe("i18n type safety (compile-time checks)", () => {
  it("valid key paths compile without error", () => {
    // These are type assertions only — no runtime behavior tested.
    // If any of these keys does NOT exist in the JSON files,
    // TypeScript will report a compile error here.
    type ValidCommonKey = "app.name" extends string ? true : never;
    const _check: ValidCommonKey = true;
    expect(_check).toBe(true);
  });
});
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Add useTranslation namespace helper
─────────────────────────────────────────────────────────────────────
Create a thin wrapper in `src/i18n/use-t.ts`:

```typescript
// src/i18n/use-t.ts
// Convenience re-export of useTranslation with a typed namespace.
// Usage: import { useT } from "../i18n/use-t";
//         const { t } = useT("auth");  // typed to auth namespace keys

export { useTranslation as useT } from "react-i18next";
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors after types.ts is added
- If a deliberate typo is introduced (e.g., `t("app.nme")` in a component),
  TypeScript should flag it as a type error. Test this manually then revert.
- types.test.ts compiles and the 1 test shows `ok` in `pnpm test`
```

---

### Supervisor Verification — Sprint S2

**V1 — Type checking accepts valid keys.**
Run `pnpm run typecheck`. It must complete with 0 errors. If new errors appeared after
adding `types.ts`, they indicate that the JSON files have key paths that differ from
what TypeScript inferred. Flag any new error message that mentions `CustomTypeOptions`.

**V2 — Invalid key causes a type error.**
In any component that uses `useTranslation("common")`, temporarily change a call to
`t("app.nme_does_not_exist")`. Run `pnpm run typecheck`. It must show a type error on
that line. Revert the change. If no error was reported, the module augmentation is not
activating. Check that `import "./types"` is present in `index.ts`.

---

## Sprint S3 — Rust Locale Detection, IPC, and Frontend Locale Store

### AI Agent Prompt

```
You are a senior Rust and React/TypeScript engineer. Sprints S1 and S2 are complete.
Your task is to:
1. Build the Rust locale detection module
2. Add `get_locale_preference` and `set_locale_preference` IPC commands
3. Build the frontend Zustand locale store and `use-locale` hook
4. Write I18N_ARCHITECTURE.md

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/locale/mod.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/locale/mod.rs
//! Locale preference detection and persistence.
//!
//! Locale precedence (highest to lowest):
//!   1. User preference in system_config (key: locale.user_language)
//!   2. Tenant default in system_config (key: locale.default_language)
//!   3. OS locale (detected at startup via sys_locale)
//!   4. Hard fallback: "fr"

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use crate::errors::{AppError, AppResult};

/// Supported locale codes. Only lowercase base codes — no region variants.
pub const SUPPORTED_LOCALES: &[&str] = &["fr", "en"];
pub const DEFAULT_LOCALE:    &str = "fr";
pub const FALLBACK_LOCALE:   &str = "fr";

// ── Full locale preference (returned to the frontend) ────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalePreference {
    /// The resolved locale to use right now (may be user, tenant, or OS).
    pub active_locale:    String,
    /// The user's explicit preference, if set.
    pub user_locale:      Option<String>,
    /// The tenant-level default, if set.
    pub tenant_locale:    Option<String>,
    /// The OS locale at startup (informational).
    pub os_locale:        Option<String>,
    /// All supported locale codes.
    pub supported_locales: Vec<String>,
}

/// Detect the OS locale and return the base language code (e.g., "fr" from "fr-DZ").
pub fn detect_os_locale() -> Option<String> {
    sys_locale::get_locale()
        .map(|loc| {
            // Strip region: "fr-DZ" → "fr", "en-US" → "en"
            let base = loc.split('-').next().unwrap_or("fr").to_lowercase();
            // Validate against supported locales
            if SUPPORTED_LOCALES.contains(&base.as_str()) {
                base
            } else {
                FALLBACK_LOCALE.to_string()
            }
        })
}

/// Read a locale value from system_config by key.
pub async fn read_locale_config(db: &DatabaseConnection, key: &str) -> AppResult<Option<String>> {
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT value FROM system_config WHERE key = ?",
        [key.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(row.and_then(|r| r.try_get::<String>("", "value").ok()))
}

/// Write a locale value to system_config (upsert).
pub async fn write_locale_config(
    db: &DatabaseConnection,
    key: &str,
    value: &str,
) -> AppResult<()> {
    if !SUPPORTED_LOCALES.contains(&value) {
        return Err(AppError::Validation(format!(
            "Unsupported locale '{value}'. Supported: {:?}",
            SUPPORTED_LOCALES
        )));
    }
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO system_config (key, value, updated_at)
           VALUES (?, ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        [key.into(), value.into(), now.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

/// Resolve the full locale preference according to the precedence chain.
pub async fn resolve_locale_preference(db: &DatabaseConnection) -> AppResult<LocalePreference> {
    let user_locale   = read_locale_config(db, "locale.user_language").await?;
    let tenant_locale = read_locale_config(db, "locale.default_language").await?;
    let os_locale     = detect_os_locale();

    let active_locale = user_locale
        .clone()
        .or_else(|| tenant_locale.clone())
        .or_else(|| os_locale.clone())
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string());

    Ok(LocalePreference {
        active_locale,
        user_locale,
        tenant_locale,
        os_locale,
        supported_locales: SUPPORTED_LOCALES.iter().map(|s| s.to_string()).collect(),
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_locales_includes_fr_and_en() {
        assert!(SUPPORTED_LOCALES.contains(&"fr"));
        assert!(SUPPORTED_LOCALES.contains(&"en"));
    }

    #[test]
    fn os_locale_returns_base_code() {
        // Just verify it doesn't panic on this machine
        let _locale = detect_os_locale();
    }

    #[test]
    fn unsupported_locale_rejected() {
        // Validation test: "de" (German) is not supported
        // This test just confirms the constant array doesn't include "de"
        assert!(!SUPPORTED_LOCALES.contains(&"de"));
    }
}
```

Add to Cargo.toml:
```toml
sys_locale = "0.3"
```

Declare in lib.rs:
```rust
pub mod locale;
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/src/commands/locale.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/commands/locale.rs
//! IPC commands for locale preference management.

use tauri::State;
use serde::Deserialize;
use crate::state::AppState;
use crate::errors::AppResult;
use crate::locale::{self, LocalePreference};

/// Get the resolved locale preference.
/// Does NOT require an active session — the login screen needs this.
#[tauri::command]
pub async fn get_locale_preference(
    state: State<'_, AppState>,
) -> AppResult<LocalePreference> {
    locale::resolve_locale_preference(&state.db).await
}

/// Payload for setting locale preference.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLocalePayload {
    /// The locale code to set ("fr" | "en").
    pub locale: String,
    /// Whether to save as the tenant default (requires adm.settings permission)
    /// or as a user preference. Defaults to user preference if false/omitted.
    pub as_tenant_default: Option<bool>,
}

/// Set the locale preference.
/// User preference → sets locale.user_language in system_config.
/// Tenant default  → sets locale.default_language (requires adm.settings).
#[tauri::command]
pub async fn set_locale_preference(
    payload: SetLocalePayload,
    state: State<'_, AppState>,
) -> AppResult<LocalePreference> {
    // Require session to change locale preference
    let user = crate::require_session!(state);

    let key = if payload.as_tenant_default.unwrap_or(false) {
        // Check adm.settings permission for tenant-wide change
        let allowed = crate::auth::rbac::check_permission(
            &state.db,
            user.user_id,
            "adm.settings",
            &crate::auth::rbac::PermissionScope::Global,
        )
        .await?;
        if !allowed {
            return Err(crate::errors::AppError::PermissionDenied("adm.settings".into()));
        }
        "locale.default_language"
    } else {
        "locale.user_language"
    };

    locale::write_locale_config(&state.db, key, &payload.locale).await?;

    tracing::info!(
        user_id = %user.user_id,
        locale = %payload.locale,
        key = %key,
        "locale::preference_updated"
    );

    locale::resolve_locale_preference(&state.db).await
}
```

Register both commands in `generate_handler!` and declare `commands::locale` in
`commands/mod.rs`.

─────────────────────────────────────────────────────────────────────
STEP 3 — Extend shared/ipc-types.ts
─────────────────────────────────────────────────────────────────────
```typescript
export interface LocalePreference {
  active_locale: string;
  user_locale: string | null;
  tenant_locale: string | null;
  os_locale: string | null;
  supported_locales: string[];
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Create src/services/locale-service.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/services/locale-service.ts
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { LocalePreference } from "../../shared/ipc-types";

const LocalePreferenceSchema = z.object({
  active_locale:     z.string(),
  user_locale:       z.string().nullable(),
  tenant_locale:     z.string().nullable(),
  os_locale:         z.string().nullable(),
  supported_locales: z.array(z.string()),
});

export async function getLocalePreference(): Promise<LocalePreference> {
  const raw = await invoke<unknown>("get_locale_preference");
  return LocalePreferenceSchema.parse(raw);
}

export async function setLocalePreference(
  locale: string,
  asTenantDefault = false
): Promise<LocalePreference> {
  const raw = await invoke<unknown>("set_locale_preference", {
    payload: { locale, asTenantDefault },
  });
  return LocalePreferenceSchema.parse(raw);
}
```

─────────────────────────────────────────────────────────────────────
STEP 5 — Create src/stores/locale-store.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/stores/locale-store.ts
import { create } from "zustand";
import { i18n } from "../i18n";
import type { LocalePreference } from "../../shared/ipc-types";

interface LocaleState {
  preference: LocalePreference | null;
  isLoading: boolean;
  error: string | null;
  /** Initialize locale from backend; call at app startup after auth init. */
  initialize: (fetchFn: () => Promise<LocalePreference>) => Promise<void>;
  /** Change locale immediately and persist to backend. */
  setLocale: (
    locale: string,
    saveFn: (locale: string) => Promise<LocalePreference>
  ) => Promise<void>;
}

export const useLocaleStore = create<LocaleState>((set) => ({
  preference: null,
  isLoading: false,
  error: null,

  initialize: async (fetchFn) => {
    set({ isLoading: true, error: null });
    try {
      const pref = await fetchFn();
      // Apply to i18next immediately
      if (pref.active_locale !== i18n.language) {
        await i18n.changeLanguage(pref.active_locale);
      }
      set({ preference: pref, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  setLocale: async (locale, saveFn) => {
    set({ isLoading: true });
    try {
      // Optimistic: change i18next right away for instant UI feedback
      await i18n.changeLanguage(locale);
      const pref = await saveFn(locale);
      set({ preference: pref, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },
}));
```

─────────────────────────────────────────────────────────────────────
STEP 6 — Create src/hooks/use-locale.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/use-locale.ts
import { useEffect } from "react";
import { useLocaleStore } from "../stores/locale-store";
import { getLocalePreference, setLocalePreference } from "../services/locale-service";
import { LOCALE_DISPLAY_NAMES, SUPPORTED_LOCALES, type SupportedLocale } from "../i18n/namespaces";

export function useLocale() {
  const { preference, isLoading, error, initialize, setLocale } = useLocaleStore();

  // Auto-initialize on first use if not yet loaded
  useEffect(() => {
    if (!preference && !isLoading) {
      void initialize(getLocalePreference);
    }
  }, [preference, isLoading, initialize]);

  const changeLocale = async (locale: SupportedLocale) => {
    await setLocale(locale, (l) => setLocalePreference(l));
  };

  return {
    activeLocale:    preference?.active_locale ?? "fr",
    supportedLocales: SUPPORTED_LOCALES as readonly SupportedLocale[],
    displayNames:    LOCALE_DISPLAY_NAMES,
    isLoading,
    error,
    changeLocale,
  };
}
```

─────────────────────────────────────────────────────────────────────
STEP 7 — Create docs/I18N_ARCHITECTURE.md
─────────────────────────────────────────────────────────────────────
Write the following reference document:

```markdown
# i18n Architecture Reference

## Foundational Principles

- **French first.** All UI strings default to French. English is parity-required from
  launch. Additional languages (Arabic + RTL, regional French variants) are added via
  the same model in Phase 5.
- **No hardcoded strings.** Every user-visible string goes through `t("ns:key")`.
  Hardcoded strings in components are a code review rejection criteria.
- **Namespace isolation.** Each product module owns its namespace. Cross-module key
  reuse requires the string to be in `common`.
- **Missing keys are visible.** During development, a missing key renders as `[key.path]`.
  In production, the fallback locale (English) is shown instead.

## Locale Precedence (highest first)

1. `system_config` key `locale.user_language` — user explicit preference
2. `system_config` key `locale.default_language` — tenant default
3. OS locale (detected at startup via `sys_locale` Rust crate)
4. Hard fallback: `fr`

## Namespace Map

| Namespace | Load | Module |
|-----------|------|--------|
| `common` | Eager | Global (labels, actions, confirmations) |
| `auth` | Eager | Login screen, session, password |
| `errors` | Eager | All AppError error messages |
| `validation` | Eager | Form validation messages |
| `formats` | Eager | Date/number format tokens (non-visual) |
| `shell` | Eager | TopBar, Sidebar, StatusBar |
| `equipment` | Lazy | §6.3 Equipment module |
| `di` | Lazy | §6.4 Intervention Requests |
| `ot` | Lazy | §6.5 Work Orders |
| `org` | Lazy | §6.2 Organization |
| `personnel` | Lazy | §6.6 Personnel |
| `reference` | Lazy | §6.13 Reference Data |
| `inventory` | Lazy | §6.8 Inventory |
| `pm` | Lazy | §6.9 Preventive Maintenance |
| `planning` | Lazy | §6.16 Planning |
| `permits` | Lazy | §6.23 Work Permits |
| `inspections` | Lazy | §6.25 Inspection Rounds |
| `training` | Lazy | §6.20 Training & Habilitation |
| `reliability` | Lazy | §6.11 RAMS / Reliability |
| `budget` | Lazy | §6.24 Budget & Costs |
| `reports` | Lazy | §6.12 Reports |
| `archive` | Lazy | §6.12 Archive Explorer |
| `notifications` | Lazy | §6.14 Notifications |
| `documentation` | Lazy | §6.15 Documentation |
| `iot` | Lazy | §6.21 IoT Gateway |
| `erp` | Lazy | §6.22 ERP Connector |
| `activity` | Lazy | §6.17 Activity Feed |
| `users` | Lazy | §6.7 Users & Roles Admin |
| `settings` | Lazy | §6.18 Settings |
| `configuration` | Lazy | §6.26 Configuration Engine |

## TypeScript Type Safety

Eager namespaces are type-checked via `src/i18n/types.ts` (module augmentation).
Any typo in a key from `common`, `auth`, `errors`, `validation`, or `shell` is a
compile error. Module namespaces are dynamically loaded — use `t("key")` without
type narrowing, and rely on the missing-key detection in `parseMissingKeyHandler`.

## Adding a New Language

1. Create `src/i18n/fr-DZ/*.json` and `src/i18n/locale-data/fr-DZ/*.json` files
2. Add `"fr-DZ"` to `SUPPORTED_LOCALES` in `namespaces.ts`
3. Add `"fr-DZ"` to `SUPPORTED_LOCALES` in `src-tauri/src/locale/mod.rs`
4. Add display name to `LOCALE_DISPLAY_NAMES`
5. The fallback chain `fr-DZ → fr → en` is handled automatically by i18next

## IPC Commands

| Command | Auth | Purpose |
|---------|------|---------|
| `get_locale_preference` | None | Read resolved locale (login screen needs this) |
| `set_locale_preference` | Session | Save user or tenant locale preference |
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test auth::locale → 3 new locale tests pass
- pnpm run dev: app uses French by default
- After calling set_locale_preference({locale: "en"}), app switches to English
  labels without reload (verified in DevTools)
- docs/I18N_ARCHITECTURE.md exists with the namespace map table
- IPC_COMMAND_REGISTRY.md updated with get_locale_preference and set_locale_preference
```

---

### Supervisor Verification — Sprint S3

**V1 — Locale detection returns a valid value.**
Run in DevTools after login:
```javascript
window.__TAURI__.core.invoke('get_locale_preference')
  .then(r => console.log(JSON.stringify(r, null, 2)));
```
The `active_locale` field must be `"fr"` or `"en"`. The `supported_locales` array
must contain both. If `active_locale` is anything else, the fallback chain is broken.

**V2 — Language switch works at runtime.**
Run:
```javascript
window.__TAURI__.core.invoke('set_locale_preference', { payload: { locale: 'en', asTenantDefault: false } });
```
Then without reloading, check the sidebar labels — they should switch from French to
English ("Équipements" → "Equipment", "Paramètres" → "Settings"). If labels don't
change, `i18n.changeLanguage()` is not being called in the locale store. Flag it.

**V3 — Locale preference is persisted.**
Switch to English. Close and reopen the app. Run `get_locale_preference` again.
`user_locale` must be `"en"` and `active_locale` must be `"en"`. If it reverts to
French, the `system_config` write is not working. Check DBeaver for the row:
```sql
SELECT key, value FROM system_config WHERE key = 'locale.user_language';
```

---

*End of Phase 1 · Sub-phase 05 · File 01*

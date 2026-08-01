# Phase 1 · Sub-phase 05 · File 04
# Translation Governance and Multilingual QA Baseline

## Context and Purpose

Files 01–03 built the complete multilingual infrastructure: the namespace registry, the
lazy-loading i18n configuration, type safety for `t()`, the Rust locale detection module,
IPC commands, the Zustand locale store, all translation JSON content for six eager
namespaces, module namespace starters for equipment/DI/OT, the pure formatter utilities,
the `useFormatters()` hook, RTL direction infrastructure, and unit tests for formatters
and fallback behavior.

What is not yet in place is the **governance layer** — the rules, tooling, and quality
gates that ensure the translation system remains correct and complete as hundreds of
additional keys are added across the 24 remaining module namespaces.

Without governance, translation quality degrades in predictable ways:
- A developer adds a feature in French and forgets to add the English key → the CI
  build passes but English users see `[equipment:detail.newField]`
- Different developers invent different key naming conventions across modules →
  `equipment_list_title` vs `equipment.list.title` vs `listTitle` all appear in the same
  codebase
- A translator changes a French string to fix a terminology issue but the English
  string remains outdated and inconsistent
- No one is sure which strings have been translated by a human vs auto-generated, so
  translation reviews are skipped

This file delivers the tooling, documentation, and CI scaffolding that prevents all four
of those failure modes. It is the "definition of done" gate for SP05.

## Architecture Rules Applied

- **The parity check script is authoritative.** No PR that adds or removes a key in one
  locale file may merge without the corresponding key being present in the other locale
  file. The CI script (not a reviewer's attention) enforces this.
- **Key naming convention is non-negotiable after SP05.** Phase 2 sprint authors must
  read `TRANSLATION_GOVERNANCE.md` before writing their first module translation file.
  Any key that does not follow the convention will be rejected by code review.
- **No free-text strings in component code.** Every user-visible string in a TSX
  component must come from `t()` or a formatter. Hardcoded strings such as
  `<h1>Équipements</h1>` are a CI violation.
- **`formats.json` is exempt from parity.** The two locale format token files are
  intentionally different. The parity script must know about this exemption.
- **Placeholder JSON files (empty `{}`) are parity-exempt.** A module that has not yet
  been translated is represented by `{}` in both locales. The parity check treats two
  matching `{}` files as passing.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `scripts/check-i18n-parity.ts` | CLI script — exits 1 if any fr/en key mismatch |
| `package.json` (patch) | Add `"i18n:check"` script |
| `docs/TRANSLATION_GOVERNANCE.md` | Key naming convention, contribution guide, review process |
| `docs/SP05_COMPLETION_CHECKLIST.md` | Gate checklist before SP05 is considered done |

## Prerequisites

- SP05-F01: namespace registry — `EAGER_NAMESPACES`, `MODULE_NAMESPACES`
- SP05-F02: all eager JSON files created, module starters created
- SP05-F03: placeholder `{}` JSON files for all 24 module namespaces

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Translation Parity Check Script | `scripts/check-i18n-parity.ts`, `package.json` patch |
| S2 | Translation Governance Documentation | `docs/TRANSLATION_GOVERNANCE.md` |
| S3 | SP05 Completion Checklist | `docs/SP05_COMPLETION_CHECKLIST.md`, final verification run |

---

## Sprint S1 — Translation Parity Check Script

### AI Agent Prompt

```
You are a Node.js / TypeScript engineer. Write a CLI script that checks translation key
parity between French and English JSON files. The script must be runnable via
`pnpm run i18n:check` and exit with code 1 if any mismatch is found, so that CI systems
can use it as a build gate.

─────────────────────────────────────────────────────────────────────
CREATE scripts/check-i18n-parity.ts
─────────────────────────────────────────────────────────────────────
The script must:
1. Check ALL eager namespace files: fr/{ns}.json vs en/{ns}.json for each of the
   6 eager namespaces. SKIP formats.json (intentionally different).
2. Check ALL module namespace files: locale-data/fr/{ns}.json vs locale-data/en/{ns}.json
   for all 24 module namespaces. Skip pairs where BOTH files are `{}` (placeholders).
   Flag pairs where one is `{}` and the other has keys (asymmetric placeholder).
3. Collect all mismatches (keys in fr but not en, keys in en but not fr) across all files.
4. Print a structured report to stdout.
5. Exit with code 0 if no mismatches. Exit with code 1 if any mismatches exist.

```typescript
#!/usr/bin/env tsx
/**
 * check-i18n-parity.ts
 *
 * Verifies that all translation JSON files in fr/ have matching key trees in en/
 * and vice versa. Skips:
 *   - formats.json (locale-specific tokens — intentionally different)
 *   - namespace pairs where BOTH files are empty objects {} (pending translation)
 *
 * Exit codes:
 *   0 — All checked namespaces are in parity
 *   1 — One or more namespaces have mismatched keys
 *
 * Usage:
 *   pnpm run i18n:check
 *   tsx scripts/check-i18n-parity.ts
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");

// ─── Configuration ──────────────────────────────────────────────────────────

const EAGER_NAMESPACES = [
  "common",
  "auth",
  "errors",
  "validation",
  "shell",
  // "formats" is intentionally excluded — it contains locale-specific tokens
];

const MODULE_NAMESPACES = [
  "equipment", "di", "ot",
  "org", "personnel", "reference",
  "inventory", "pm", "planning",
  "permits", "inspections", "training",
  "reliability", "budget", "reports",
  "archive", "notifications", "documentation",
  "iot", "erp", "activity",
  "users", "settings", "configuration",
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

type JsonObject = Record<string, unknown>;

function readJson(filePath: string): JsonObject | null {
  if (!fs.existsSync(filePath)) return null;
  try {
    const raw = fs.readFileSync(filePath, "utf-8");
    return JSON.parse(raw) as JsonObject;
  } catch (err) {
    console.error(`  ✗ Invalid JSON: ${path.relative(ROOT, filePath)}`);
    console.error(`    ${String(err)}`);
    return null;
  }
}

function collectLeafKeys(obj: JsonObject, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value)
    ) {
      return collectLeafKeys(value as JsonObject, fullKey);
    }
    return [fullKey];
  });
}

function isEmpty(obj: JsonObject): boolean {
  return Object.keys(obj).length === 0;
}

// ─── Check logic ──────────────────────────────────────────────────────────────

interface ParityResult {
  namespace: string;
  skipped: boolean;
  skipReason?: string;
  missingInEn: string[];
  missingInFr: string[];
  parseError: boolean;
}

function checkParity(
  namespace: string,
  frPath: string,
  enPath: string
): ParityResult {
  const frJson = readJson(frPath);
  const enJson = readJson(enPath);

  if (frJson === null || enJson === null) {
    return {
      namespace,
      skipped: false,
      missingInEn: [],
      missingInFr: [],
      parseError: true,
    };
  }

  if (isEmpty(frJson) && isEmpty(enJson)) {
    return {
      namespace,
      skipped: true,
      skipReason: "both files are empty placeholders {}",
      missingInEn: [],
      missingInFr: [],
      parseError: false,
    };
  }

  const frKeys = new Set(collectLeafKeys(frJson));
  const enKeys = new Set(collectLeafKeys(enJson));

  const missingInEn = [...frKeys].filter((k) => !enKeys.has(k)).sort();
  const missingInFr = [...enKeys].filter((k) => !frKeys.has(k)).sort();

  return {
    namespace,
    skipped: false,
    missingInEn,
    missingInFr,
    parseError: false,
  };
}

// ─── Main ─────────────────────────────────────────────────────────────────────

function main(): void {
  console.log("╔══════════════════════════════════════════════════╗");
  console.log("║   Maintafox — Translation Parity Check           ║");
  console.log("╚══════════════════════════════════════════════════╝");
  console.log("");

  const results: ParityResult[] = [];
  let hasErrors = false;

  // Check eager namespace files
  console.log("─── Eager namespaces ──────────────────────────────");
  for (const ns of EAGER_NAMESPACES) {
    const frPath = path.join(ROOT, "src", "i18n", "fr", `${ns}.json`);
    const enPath = path.join(ROOT, "src", "i18n", "en", `${ns}.json`);
    const result = checkParity(ns, frPath, enPath);
    results.push(result);

    if (result.parseError) {
      console.log(`  ✗ ${ns.padEnd(20)} parse error (see above)`);
      hasErrors = true;
    } else if (result.skipped) {
      console.log(`  ○ ${ns.padEnd(20)} skipped — ${result.skipReason}`);
    } else if (result.missingInEn.length === 0 && result.missingInFr.length === 0) {
      console.log(`  ✓ ${ns.padEnd(20)} OK`);
    } else {
      console.log(`  ✗ ${ns.padEnd(20)} MISMATCH`);
      hasErrors = true;
    }
  }

  // Check module namespace files
  console.log("");
  console.log("─── Module namespaces ─────────────────────────────");
  for (const ns of MODULE_NAMESPACES) {
    const frPath = path.join(ROOT, "src", "i18n", "locale-data", "fr", `${ns}.json`);
    const enPath = path.join(ROOT, "src", "i18n", "locale-data", "en", `${ns}.json`);
    const result = checkParity(ns, frPath, enPath);
    results.push(result);

    if (result.parseError) {
      console.log(`  ✗ ${ns.padEnd(20)} parse error (see above)`);
      hasErrors = true;
    } else if (result.skipped) {
      console.log(`  ○ ${ns.padEnd(20)} placeholder (both empty)`);
    } else if (result.missingInEn.length === 0 && result.missingInFr.length === 0) {
      console.log(`  ✓ ${ns.padEnd(20)} OK`);
    } else {
      console.log(`  ✗ ${ns.padEnd(20)} MISMATCH`);
      hasErrors = true;
    }
  }

  // Detailed error report
  const errored = results.filter(
    (r) => !r.skipped && !r.parseError && (r.missingInEn.length > 0 || r.missingInFr.length > 0)
  );

  if (errored.length > 0) {
    console.log("");
    console.log("─── Mismatch details ──────────────────────────────");
    for (const r of errored) {
      console.log(`\n  Namespace: ${r.namespace}`);
      if (r.missingInEn.length > 0) {
        console.log(`  Missing in en/ (${r.missingInEn.length} keys):`);
        r.missingInEn.forEach((k) => console.log(`    - ${k}`));
      }
      if (r.missingInFr.length > 0) {
        console.log(`  Missing in fr/ (${r.missingInFr.length} keys):`);
        r.missingInFr.forEach((k) => console.log(`    + ${k}`));
      }
    }
  }

  // Summary
  const checked = results.filter((r) => !r.skipped && !r.parseError);
  const passed = checked.filter((r) => r.missingInEn.length === 0 && r.missingInFr.length === 0);
  const skipped = results.filter((r) => r.skipped);
  const parseErrors = results.filter((r) => r.parseError);

  console.log("");
  console.log("─── Summary ───────────────────────────────────────");
  console.log(`  Checked:      ${checked.length}`);
  console.log(`  Passed:       ${passed.length}`);
  console.log(`  Mismatches:   ${checked.length - passed.length}`);
  console.log(`  Skipped:      ${skipped.length} (placeholder pairs)`);
  console.log(`  Parse errors: ${parseErrors.length}`);
  console.log("");

  if (hasErrors) {
    console.log("  ✗ Parity check FAILED. Fix mismatches before merging.");
    process.exit(1);
  } else {
    console.log("  ✓ All translation files are in parity.");
    process.exit(0);
  }
}

main();
```

─────────────────────────────────────────────────────────────────────
PATCH package.json — add the i18n:check script
─────────────────────────────────────────────────────────────────────
Open `package.json`. In the "scripts" section, add:
```json
"i18n:check": "tsx scripts/check-i18n-parity.ts"
```

Add it alongside (not replacing) the existing test/typecheck/dev/build scripts.

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm run i18n:check` runs and exits with code 0 when all eager namespace files
  are in parity
- Output shows "✓" for common, auth, errors, validation, shell
- Output shows "○" (placeholder) for all module namespaces that are `{}`
- Output shows "✓" for equipment, di, ot (which have real content in parity)
- When a key is deliberately removed from `en/common.json`, the script exits with
  code 1 and prints the missing key path
- `tsx scripts/check-i18n-parity.ts` runs without requiring a build step
```

---

### Supervisor Verification — Sprint S1

**V1 — Script runs clean on current state.**
Run `pnpm run i18n:check`. Expected output:
- 5 eager namespaces show "✓" (formats.json is excluded)
- equipment, di, ot show "✓" (real content, in parity from F02)
- Remaining 21 module namespaces show "○ placeholder"
- Summary: "✓ All translation files are in parity."
- Exit code 0

**V2 — Script catches a deliberate parity break.**
In `src/i18n/en/common.json`, add a test key:
```json
"action": { ..., "deliberateTestKey": "Test" }
```
Run `pnpm run i18n:check`. The script MUST now exit with code 1 and print:
```
  Missing in fr/ (1 keys):
    + action.deliberateTestKey
```
Remove the test key and restore parity. Rerun — exit code must be 0 again.

**V3 — CI integration plan.**
In `.github/workflows/ci.yml` (or the equivalent CI config in `Makefile` or
`docker-compose.yml`), confirm that `pnpm run i18n:check` is included in the CI
pipeline step that runs before the build. If no CI config exists yet, document this
in `docs/SP05_COMPLETION_CHECKLIST.md` as a required step for the CI sub-phase.

---

## Sprint S2 — Translation Governance Documentation

### AI Agent Prompt

```
You are a technical writer and multilingual software engineer. Write the translation
governance documentation for the Maintafox project. This document will be read by all
developers before they write their first translation key and by external translators
before they review or update translation files.

─────────────────────────────────────────────────────────────────────
CREATE docs/TRANSLATION_GOVERNANCE.md
─────────────────────────────────────────────────────────────────────

# Translation Governance

## 1. Overview

Maintafox is a bilingual French/English industrial maintenance platform. French is the
primary locale and default display language. English is the fallback locale. A third
locale (Arabic, ar-DZ) is planned for Phase 3.

All user-visible strings in the application must come from translation JSON files —
never from hardcoded text in TypeScript/TSX component code.

This document defines the rules that all contributors must follow when adding,
modifying, or reviewing translation strings.

---

## 2. File Structure

```
src/i18n/
├── fr/                          ← Eager namespace files (French)
│   ├── common.json
│   ├── auth.json
│   ├── errors.json
│   ├── validation.json
│   ├── formats.json             ← NOT user strings — locale tokens only
│   └── shell.json
├── en/                          ← Eager namespace files (English)
│   └── [same 6 files]
└── locale-data/
    ├── fr/                      ← Module namespace files (French, lazy-loaded)
    │   ├── equipment.json
    │   ├── di.json
    │   └── [24 files total]
    └── en/                      ← Module namespace files (English, lazy-loaded)
        └── [24 files total]
```

Eager namespaces are bundled at startup and available before any module loads.
Module namespaces are loaded on demand when the user first visits that module.

---

## 3. Key Naming Convention

### 3.1 Structure

All keys follow dot-notation hierarchical naming:

```
{scope}.{element}[.{sub-element}[.{variant}]]
```

**Scope** is the top-level context grouping:
- For eager namespaces, scope reflects the domain: `action`, `status`, `label`, etc.
- For module namespaces, scope reflects the view: `page`, `list`, `detail`, `form`,
  `status`, `action`, `empty`, `filter`

**Element** is the specific object within that scope.

### 3.2 Rules

| Rule | Correct | Incorrect |
|------|---------|-----------|
| Use dot-notation hierarchy | `action.save` | `action_save`, `actionSave` |
| Use camelCase for sub-elements | `form.fieldName.label` | `form.field_name.label` |
| Use lowercase for scope | `equipment.list.title` | `Equipment.List.Title` |
| Be descriptive, not abbreviated | `auth.login.username.label` | `auth.usr.lbl` |
| No HTML in values | Use Trans component | `"Save <b>now</b>"` |
| No whitespace-only values | `"Save"` | `"  "` |

### 3.3 Standard Sub-elements (apply across all modules)

| Sub-element | Purpose | Example |
|-------------|---------|---------|
| `.title` | Page or section heading | `equipment.page.title` = "Équipements" |
| `.label` | Form field label | `equipment.form.code.label` = "Code" |
| `.placeholder` | Input placeholder | `equipment.form.code.placeholder` = "EQ-001" |
| `.hint` | Input helper text below field | `equipment.form.code.hint` = "Code unique..." |
| `.tooltip` | Hover tooltip | `equipment.action.createDI.tooltip` |
| `.error` | Field-level validation message | `equipment.form.code.error.required` |
| `_one` | Singular (i18next plural) | `equipment.list.count_one` = "1 équipement" |
| `_other` | Plural (i18next plural) | `equipment.list.count_other` = "{{count}} équipements" |

### 3.4 Status Code Mapping

Status values stored in the database (e.g., `"operational"`) are mapped to
display labels via the `{module}.status.{value}` key:

```json
// equipment.json
{
  "status": {
    "operational":   "En service",
    "maintenance":   "En maintenance",
    "decommissioned":"Déclassé"
  }
}
```

Usage in code: `t('status.operational', { ns: 'equipment' })`

Status codes MUST match exactly the values stored in the database (see the
`equipment_status` enum in the schema). The translation key is the database value.

---

## 4. How to Add a New Key

### Step 1 — Identify the correct namespace

If the string appears only in one module (equipment page, work order form), use that
module's namespace (`equipment`, `ot`, etc.). If it appears globally (action button,
generic error), use an eager namespace (`common`, `errors`, etc.).

### Step 2 — Choose the key path following §3

Bad: `equipment.equipmentCodeLabel` ← flat, not hierarchical
Good: `equipment.form.code.label` ← hierarchy: module → section → field → type

### Step 3 — Add to BOTH fr/ and en/ simultaneously

Never add to fr without adding to en. The CI parity check will block the PR, but it
is better practice to write both at the same time.

### Step 4 — Use the key in code via the hook

```tsx
// In a component inside the equipment module
const { t } = useT("equipment");
return <label>{t("form.code.label")}</label>;
```

For eager namespaces:
```tsx
const { t } = useT("common");
return <button>{t("action.save")}</button>;
```

### Step 5 — Run parity check locally

```bash
pnpm run i18n:check
```

If it exits with code 0, the keys are in parity. If code 1, add the missing key.

---

## 5. How to Add a New Module Namespace

A new module namespace is added when Phase 2 begins work on a module that does not yet
have a translation file.

1. Add the namespace to `MODULE_NAMESPACES` in `src/i18n/namespaces.ts`.
   The key must be the module code (e.g., `"inventory"`), the value the namespace
   name (typically the same string).
2. Create `src/i18n/locale-data/fr/{ns}.json` and
   `src/i18n/locale-data/en/{ns}.json` with at minimum the page title keys.
3. Never leave one side empty and the other populated — even if the namespace has
   only two keys, both files must have those two keys.

---

## 6. Updating Existing Translations

All translation updates must go through a code review that includes at minimum one
reviewer with professional-level proficiency in the target language.

When updating a French string:
1. Consider whether the equivalent English string needs to be updated for consistency.
2. If the change is a terminology correction (e.g., "Demande d'intervention" →
   "Fiche de panne"), search the codebase for all uses of the old key value to
   identify any documentation or screenshots that need updating.
3. Do NOT change a key path to fix a translation — fix the value, not the key. Changing
   key paths breaks all existing `t()` calls throughout the codebase.

---

## 7. Tenant Term Overrides

The database schema includes a `term_overrides` table (§6.26 of the PRD):
- `locale` (e.g., `fr-DZ`, `en-US`, `ar-DZ`)
- `i18n_key` (e.g., `equipment.page.title`)
- `default_value` (the JSON file value, stored for reference)
- `override_value` (the tenant-specific replacement)

The override lookup is handled by the backend at runtime — the frontend translation
layer sees the override as if it were the default value. This means:
- Keys in JSON files represent the default, unoverridden values
- Tenant admins use the administration UI (§6.13, §6.18) to set overrides
- Translation JSON files do NOT change when a tenant sets an override
- The parity check does not need to validate overrides (they are DB-stored)

---

## 8. Forbidden Patterns

The following patterns are caught by code review and will be rejected:

| Forbidden | Reason | Correct Alternative |
|-----------|--------|---------------------|
| Hardcoded string in JSX: `<h1>Équipements</h1>` | Not translatable | `<h1>{t("page.title")}</h1>` |
| String concatenation: `"Équipement " + name` | Cannot be translated differently depending on locale grammar | `t("equipment.detail.title", { name })` |
| Importing raw JSON and accessing it directly | Bypasses namespace lazy-loading | Use `t()` via `useT()` |
| HTML in JSON values: `"Save <b>now</b>"` | XSS risk | Use `<Trans>` component |
| Locale-specific logic in components: `if (locale === 'fr') ... else ...` | Logic should be in translations/formatters | Move to `t()` interpolation or formatter |
| Using `i18n.t()` directly in a component | Doesn't re-render on locale change | Use `useT()` hook |

---

## 9. formats.json — Special Handling

`src/i18n/fr/formats.json` and `src/i18n/en/formats.json` contain locale-specific
formatting tokens. They are consumed by `useFormatters()` and are NOT user-visible
strings. Rules:

- Their keys intentionally differ between locales (e.g., `number.decimal` is `","` in
  fr and `"."` in en)
- They are excluded from the parity check
- Never use these files for display strings
- Changes to these files require testing the formatter unit tests

---

## 10. CI Enforcement

The following checks run on every PR that touches any file in `src/i18n/`:

| Check | Command | Failure action |
|-------|---------|----------------|
| JSON validity | `pnpm run typecheck` | Fix invalid JSON syntax |
| Parity | `pnpm run i18n:check` | Add missing key to the other locale |
| TypeScript types | `pnpm run typecheck` | Update `types.ts` if eager namespace shape changed |
| Formatter tests | `pnpm test` | Fix formatter utility if locale-specific output changed |

```

---

### Supervisor Verification — Sprint S2

**V1 — Document is complete and correctly formatted:**
Run `pnpm exec markdownlint docs/TRANSLATION_GOVERNANCE.md` (if markdownlint is
installed). If not installed, manually verify:
- All section headers are present (§2 through §10)
- All tables render correctly (no broken pipe characters)
- All code fences are closed
- No broken internal links

**V2 — Key naming examples match actual files:**
Check that at least three key examples mentioned in the documentation
(`equipment.list.title`, `auth.login.username.label`, `action.save`) actually exist in
the corresponding JSON files created in F02. If a documented example key doesn't exist
in the JSON, either update the JSON or update the documentation so they stay in sync.

---

## Sprint S3 — SP05 Completion Checklist

### AI Agent Prompt

```
You are a quality engineer. Write the SP05 completion checklist that must be signed off
before Sub-phase 05 (Multilingual Foundation) is considered complete and before SP06
work may begin.

Also run the following final verification sequence and document any failures:

1. `pnpm run typecheck` — record pass/fail
2. `pnpm run i18n:check` — record pass/fail and output
3. `pnpm test src/__tests__/utils/formatters.test.ts` — record pass/fail
4. `pnpm test src/__tests__/i18n/` — record pass/fail
5. `pnpm run dev` — verify no "[ns:key]" patterns on the login screen or shell

─────────────────────────────────────────────────────────────────────
CREATE docs/SP05_COMPLETION_CHECKLIST.md
─────────────────────────────────────────────────────────────────────

# SP05 — Multilingual Foundation: Completion Checklist

## Purpose

This checklist MUST be completed before Sub-phase 05 is marked done and before any
Phase 1 Sub-phase 06 work begins. Each item must be signed off by a developer who
verifies the actual output, not assumed.

---

## Section 1 — Namespace Registry

| Item | Expected | Status |
|------|----------|--------|
| `EAGER_NAMESPACES.length` | 6 | ☐ |
| `MODULE_NAMESPACES.length` | 24 | ☐ |
| `ALL_NAMESPACES.length` | 30 | ☐ |
| `SUPPORTED_LOCALES` | `["fr", "en"]` | ☐ |
| `DEFAULT_LOCALE` | `"fr"` | ☐ |
| `FALLBACK_LOCALE` | `"en"` | ☐ |

**Verification command:**
```bash
pnpm exec tsx -e "
import { EAGER_NAMESPACES, MODULE_NAMESPACES, SUPPORTED_LOCALES } from './src/i18n/namespaces';
console.log('Eager:', EAGER_NAMESPACES.length);
console.log('Module:', MODULE_NAMESPACES.length);
console.log('Total:', EAGER_NAMESPACES.length + MODULE_NAMESPACES.length);
console.log('Locales:', SUPPORTED_LOCALES);
"
```

---

## Section 2 — Eager Namespace JSON Files

| File | Exists | Valid JSON | Non-empty | Status |
|------|--------|-----------|-----------|--------|
| `src/i18n/fr/common.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/common.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/fr/auth.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/auth.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/fr/errors.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/errors.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/fr/validation.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/validation.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/fr/formats.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/formats.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/fr/shell.json` | ☐ | ☐ | ☐ | ☐ |
| `src/i18n/en/shell.json` | ☐ | ☐ | ☐ | ☐ |

**Verification command:**
```bash
Get-ChildItem src/i18n/fr, src/i18n/en -Filter "*.json" |
  ForEach-Object {
    $content = Get-Content $_.FullName -Raw
    $obj = $content | ConvertFrom-Json
    $keys = ($obj | Get-Member -MemberType NoteProperty).Count
    Write-Host "$($_.Name): $keys top-level keys"
  }
```

---

## Section 3 — Module Namespace Files

| Check | Expected | Status |
|-------|----------|--------|
| 27 files exist in `locale-data/fr/` | equipment, di, ot + 24 others | ☐ |
| 27 files exist in `locale-data/en/` | equipment, di, ot + 24 others | ☐ |
| equipment fr/en parity | In parity | ☐ |
| di fr/en parity | In parity | ☐ |
| ot fr/en parity | In parity | ☐ |
| All remaining 24 namespaces | Empty `{}` placeholders | ☐ |

**Verification command:**
```bash
pnpm run i18n:check
```
Expected exit code: 0. All module namespaces should show "○ placeholder" or "✓".

---

## Section 4 — Locale IPC Commands

| Check | Expected | Status |
|-------|----------|--------|
| `get_locale_preference` registered in Tauri | Returns preference object | ☐ |
| `set_locale_preference` registered in Tauri | Persists preference | ☐ |
| `get_locale_preference` works on login screen (no session required) | Returns locale | ☐ |
| Preference persists across app restart | Same locale after restart | ☐ |
| OS locale detection returns base code | `"fr"` not `"fr-FR"` or `"fr-DZ"` | ☐ |

**Verification:**
In `pnpm run dev`, open DevTools → Console and run:
```javascript
await window.__TAURI__.invoke('get_locale_preference')
// Expected: { active_locale: "fr", user_locale: null, tenant_locale: null, os_locale: "fr", supported_locales: ["fr", "en"] }
```
(If `window.__TAURI__` is not exposed in dev mode, use the app's locale service:
`localeService.getLocalePreference().then(console.log)`)

---

## Section 5 — Formatter Utilities

| Check | Expected | Status |
|-------|----------|--------|
| `formatNumber(1234.56, "fr")` | `"1 234,56"` (thousands: narrow space) | ☐ |
| `formatNumber(1234.56, "en")` | `"1,234.56"` (thousands: comma) | ☐ |
| `formatDate(new Date(2026,2,31), "fr")` | `"31/03/2026"` | ☐ |
| `formatDate(new Date(2026,2,31), "en")` | `"03/31/2026"` | ☐ |
| `formatCurrency(1234.56, "fr", "EUR")` | `€` after number | ☐ |
| `formatCurrency(1234.56, "en", "USD")` | `$` before number | ☐ |
| `getLocaleDirection("fr")` | `"ltr"` | ☐ |
| `getLocaleDirection("ar")` | `"rtl"` | ☐ |
| `formatNumber(NaN, "fr")` | `"—"` (em dash, not empty) | ☐ |

**Verification command:**
```bash
pnpm test src/__tests__/utils/formatters.test.ts
```
Expected: All tests pass. Zero test failures.

---

## Section 6 — Fallback Chain Behavior

| Check | Expected | Status |
|-------|----------|--------|
| Key exists in fr → shows fr string | ✓ | ☐ |
| Key missing in fr, exists in en → shows en string | ✓ | ☐ |
| Key missing in BOTH locales → shows `[key]`, not empty string | ✓ | ☐ |
| No key shows empty string anywhere in app | ✓ | ☐ |

**Verification command:**
```bash
pnpm test src/__tests__/i18n/fallback.test.ts
```
Expected: All tests pass.

---

## Section 7 — Translation Parity

| Check | Expected | Status |
|-------|----------|--------|
| `pnpm run i18n:check` exit code | 0 | ☐ |
| common fr ↔ en parity | ✓ | ☐ |
| auth fr ↔ en parity | ✓ | ☐ |
| errors fr ↔ en parity | ✓ | ☐ |
| validation fr ↔ en parity | ✓ | ☐ |
| shell fr ↔ en parity | ✓ | ☐ |
| equipment fr ↔ en parity | ✓ | ☐ |
| di fr ↔ en parity | ✓ | ☐ |
| ot fr ↔ en parity | ✓ | ☐ |

---

## Section 8 — TypeScript Compilation

| Check | Expected | Status |
|-------|----------|--------|
| `pnpm run typecheck` exit code | 0 | ☐ |
| No errors in `src/i18n/types.ts` | 0 errors | ☐ |
| No errors in `src/i18n/config.ts` | 0 errors | ☐ |
| No errors in `src/utils/formatters.ts` | 0 errors | ☐ |
| No errors in `src/hooks/use-formatters.ts` | 0 errors | ☐ |
| No errors in `src/stores/locale-store.ts` | 0 errors | ☐ |

**Verification command:**
```bash
pnpm run typecheck
```
Expected: No errors. Zero warnings about implicit `any` in i18n-related files.

---

## Section 9 — RTL Infrastructure

| Check | Expected | Status |
|-------|----------|--------|
| `document.documentElement.dir` | `"ltr"` (fr and en) | ☐ |
| `document.documentElement.lang` | `"fr"` (default) | ☐ |
| `LocaleHtmlDir` component mounted in App root | ✓ | ☐ |
| `getLocaleDirection("ar")` returns `"rtl"` | ✓ (Phase 3 ready) | ☐ |

**Verification:**
Open DevTools → Elements in `pnpm run dev`. The `<html>` element must have both
`dir="ltr"` and `lang="fr"`. Switch locale to English — `lang` must change to `"en"`
within 1 second without a page reload.

---

## Section 10 — Visual Verification (No Fallback Key Strings)

| Screen | Check | Status |
|--------|-------|--------|
| Login screen | No `[auth:...]` patterns visible | ☐ |
| Session locked overlay | No `[auth:session.idleLocked]` visible | ☐ |
| App shell sidebar | No `[shell:sidebar.nav...]` visible | ☐ |
| App shell top bar | No `[shell:topBar...]` visible | ☐ |
| Status bar | No `[shell:statusBar...]` visible | ☐ |
| Equipment list (if component exists) | No `[equipment:...]` visible | ☐ |

**How to check:** Search the rendered DOM for `[` characters using DevTools → Elements
search (Ctrl+F in Elements). Any match points to a missing translation key.

---

## Sign-off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Developer | | | ☐ |
| Reviewer | | | ☐ |

**SP05 is considered complete when all 40 checklist items are marked ✓ and both
sign-off rows are completed.**

```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `docs/SP05_COMPLETION_CHECKLIST.md` exists and contains all 10 sections
- Running through the checklist manually: every section returns the expected result
- `pnpm run i18n:check` exits with code 0
- `pnpm run typecheck` exits with code 0
- All formatter tests pass
- All fallback tests pass
```

---

### Supervisor Verification — Sprint S3

**V1 — Full test suite passes.**
Run `pnpm test`. The following test files must all pass with 0 failures:
- `src/__tests__/utils/formatters.test.ts`
- `src/__tests__/i18n/fallback.test.ts`
- `src/i18n/__tests__/parity.test.ts`
- `src/i18n/__tests__/json-valid.test.ts`
- `src/__tests__/auth/*.test.ts` (SP04 tests — must not regress)

**V2 — Parity check is clean.**
Run `pnpm run i18n:check`. Exit code 0. Print the full output and confirm:
- 5 eager namespaces show "✓"
- equipment, di, ot show "✓"
- 21 remaining module namespaces show "○ placeholder"
- Summary counts match (8 checked, 8 passed, 21 skipped, 0 mismatches)

**V3 — TypeScript clean.**
Run `pnpm run typecheck`. Zero errors. If any error relates to a module path
(e.g., "Cannot find module './fr/common.json'"), verify that the JSON files were
created in the correct directory paths as specified in F02 and that `tsconfig.json`
has `"resolveJsonModule": true`.

**V4 — Checklist sections are actionable.**
Read through each section of `SP05_COMPLETION_CHECKLIST.md`. Every verification
command listed must be runnable without modification in the project root. Every
expected value must match the actual system state at the time of SP05 completion.
If any expected value is wrong (e.g., namespace count changed), update the checklist
to match reality — the checklist documents actual behavior, not aspirational behavior.

---

## SP05 Summary

Sub-phase 05 (Multilingual Foundation: French and English Locale Architecture) is now
fully specified across four files:

| File | Delivers | Lines |
|------|----------|-------|
| F01 — i18n Architecture and Locale Resource Model | Namespace registry, i18next config, TypeScript types, Rust locale module, IPC commands, Zustand store, React hook | ~837 |
| F02 — French and English Foundation Pack | 12 eager namespace JSON files (6 × fr + en), 6 module namespace starters, parity tests | ~650 |
| F03 — Locale-Aware Formatting and Fallback Behavior | `formatters.ts`, `useFormatters()` hook, `LocaleHtmlDir`, 18 formatter tests, 6 fallback tests, 42 module placeholder JSON files | ~560 |
| F04 — Translation Governance and Multilingual QA Baseline | `check-i18n-parity.ts` CLI script, `TRANSLATION_GOVERNANCE.md`, `SP05_COMPLETION_CHECKLIST.md` | ~430 |

**Phase 2 entry requirement:** All 40 items in `SP05_COMPLETION_CHECKLIST.md` must
be verified before any Phase 2 sub-phase sprint is started. Phase 2 sprints that
add module translation keys must run `pnpm run i18n:check` before opening a PR.

---

*End of Phase 1 · Sub-phase 05 · File 04*

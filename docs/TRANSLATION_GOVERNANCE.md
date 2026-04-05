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
├── use-t.ts                     ← Re-exports useTranslation as useT
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
| Use camelCase for sub-elements | `form.identity.code.label` | `form.identity.code_label` |
| Use lowercase for scope | `list.columns.code` | `List.Columns.Code` |
| Be descriptive, not abbreviated | `login.form.username.label` | `login.usr.lbl` |
| No HTML in values | Use Trans component | `"Save <b>now</b>"` |
| No whitespace-only values | `"Enregistrer"` | `"  "` |

### 3.3 Standard Sub-elements (apply across all modules)

| Sub-element | Purpose | Example |
|-------------|---------|---------|
| `.title` | Page or section heading | `page.title` = "Équipements" (in equipment ns) |
| `.label` | Form field label | `form.identity.code.label` = "Code équipement" |
| `.placeholder` | Input placeholder | `form.identity.code.placeholder` = "EQ-001" |
| `.hint` | Input helper text below field | `form.identity.code.hint` = "Code unique..." |
| `.tooltip` | Hover tooltip | `action.createDI` (contextual tooltip) |
| `.error` | Field-level validation message | `field.code.error.required` |
| `_one` | Singular (i18next plural) | `time.minutes_ago_one` = "il y a 1 minute" |
| `_other` | Plural (i18next plural) | `time.minutes_ago_other` = "il y a {{count}} minutes" |

### 3.4 Status Code Mapping

Status values stored in the database (e.g., `"operational"`) are mapped to
display labels via the `status.{value}` key within the module namespace:

```json
// locale-data/fr/equipment.json → "status" section
{
  "status": {
    "operational":     "En service",
    "maintenance":     "En maintenance",
    "decommissioned":  "Déclassé",
    "standby":         "En attente",
    "scrapped":        "Mis au rebut"
  }
}
```

Usage in code: `t('status.operational')` (with `useT('equipment')`)

Status codes MUST match exactly the values stored in the database (see the
`equipment_status` enum in the schema). The translation key is the database value.

---

## 4. How to Add a New Key

### Step 1 — Identify the correct namespace

If the string appears only in one module (equipment page, work order form), use that
module's namespace (`equipment`, `ot`, etc.). If it appears globally (action button,
generic error), use an eager namespace (`common`, `errors`, etc.).

### Step 2 — Choose the key path following §3

Bad: `equipmentCodeLabel` ← flat, not hierarchical
Good: `form.identity.code.label` ← hierarchy: section → group → field → type

### Step 3 — Add to BOTH fr/ and en/ simultaneously

Never add to fr without adding to en. The CI parity check will block the PR, but it
is better practice to write both at the same time.

### Step 4 — Use the key in code via the hook

```tsx
// In a component inside the equipment module
const { t } = useT("equipment");
return <label>{t("form.identity.code.label")}</label>;
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
have a translation file with real content.

1. Add the namespace to `MODULE_NAMESPACES` in `src/i18n/namespaces.ts`.
   The key must be the module code (e.g., `"inventory"`), the value the namespace
   name (typically the same string).
2. Create `src/i18n/locale-data/fr/{ns}.json` and
   `src/i18n/locale-data/en/{ns}.json` with at minimum the `page.title` key.
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
| String concatenation: `"Équipement " + name` | Cannot be translated differently depending on locale grammar | `t("detail.fields.name", { name })` |
| Importing raw JSON and accessing it directly | Bypasses namespace lazy-loading | Use `t()` via `useT()` |
| HTML in JSON values: `"Save <b>now</b>"` | XSS risk | Use `<Trans>` component |
| Locale-specific logic in components: `if (locale === 'fr') ... else ...` | Logic should be in translations/formatters | Move to `t()` interpolation or `useFormatters()` |
| Using `i18n.t()` directly in a component | Doesn't re-render on locale change | Use `useT()` hook |

---

## 9. formats.json — Special Handling

`src/i18n/fr/formats.json` and `src/i18n/en/formats.json` contain locale-specific
formatting tokens. They are consumed by `useFormatters()` and are NOT user-visible
strings. Rules:

- Their keys intentionally differ between locales (e.g., `number.decimal` is `","` in
  fr and `"."` in en, `currency.pattern` is `"{{amount}} {{symbol}}"` in fr and
  `"{{symbol}}{{amount}}"` in en)
- They are excluded from the parity check
- Never use these files for display strings
- Changes to these files require running the formatter unit tests:
  ```bash
  pnpm test src/__tests__/utils/formatters.test.ts
  ```

---

## 10. CI Enforcement

The following checks run on every PR that touches any file in `src/i18n/`:

| Check | Command | Failure action |
|-------|---------|----------------|
| JSON validity | `pnpm run typecheck` | Fix invalid JSON syntax |
| Parity | `pnpm run i18n:check` | Add missing key to the other locale |
| TypeScript types | `pnpm run typecheck` | Update `types.ts` if eager namespace shape changed |
| Formatter tests | `pnpm test` | Fix formatter utility if locale-specific output changed |

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
| `MODULE_NAMESPACES` key count | 24 | ☐ |
| Total namespace count | 30 | ☐ |
| `SUPPORTED_LOCALES` | `["fr", "en"]` | ☐ |
| `DEFAULT_LOCALE` (in config.ts) | `"fr"` | ☐ |
| `FALLBACK_LOCALE` (in config.ts) | `"en"` | ☐ |

**Verification command:**
```powershell
pnpm exec tsx -e "
import { EAGER_NAMESPACES, MODULE_NAMESPACES, ALL_NAMESPACES, SUPPORTED_LOCALES } from './src/i18n/namespaces';
console.log('Eager:', EAGER_NAMESPACES.length);
console.log('Module:', Object.keys(MODULE_NAMESPACES).length);
console.log('Total:', ALL_NAMESPACES.length);
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
```powershell
Get-ChildItem src/i18n/fr, src/i18n/en -Filter "*.json" |
  ForEach-Object {
    $content = Get-Content $_.FullName -Raw
    $obj = $content | ConvertFrom-Json
    $keys = ($obj | Get-Member -MemberType NoteProperty).Count
    Write-Host "$($_.Directory.Name)/$($_.Name): $keys top-level keys"
  }
```

---

## Section 3 — Module Namespace Files

| Check | Expected | Status |
|-------|----------|--------|
| Files in `locale-data/fr/` | 24 | ☐ |
| Files in `locale-data/en/` | 24 | ☐ |
| equipment fr/en parity | In parity | ☐ |
| di fr/en parity | In parity | ☐ |
| ot fr/en parity | In parity | ☐ |
| Remaining 21 namespaces | Empty `{}` placeholders | ☐ |

**Verification command:**
```powershell
pnpm run i18n:check
```
Expected exit code: 0. Module namespaces show "○ placeholder" (21) or "✓" (3).

---

## Section 4 — Locale IPC Commands

| Check | Expected | Status |
|-------|----------|--------|
| `get_locale_preference` registered in Tauri (`src-tauri/src/commands/locale.rs`) | Returns preference object | ☐ |
| `set_locale_preference` registered in Tauri | Persists preference | ☐ |
| `get_locale_preference` works on login screen (no session required) | Returns locale | ☐ |
| Preference persists across app restart | Same locale after restart | ☐ |
| OS locale detection returns base code | `"fr"` not `"fr-FR"` or `"fr-DZ"` | ☐ |

**Verification:**
In `pnpm run dev`, open DevTools → Console:
```javascript
await window.__TAURI__.invoke('get_locale_preference')
// Expected: { active_locale: "fr", user_locale: null, tenant_locale: null, os_locale: "fr", supported_locales: ["fr", "en"] }
```

---

## Section 5 — Formatter Utilities

| Check | Expected | Status |
|-------|----------|--------|
| `formatNumber(1234.56, "fr")` | `"1 234,56"` (narrow no-break space) | ☐ |
| `formatNumber(1234.56, "en")` | `"1,234.56"` | ☐ |
| `formatDate(new Date(2026,2,31), "fr")` | `"31/03/2026"` | ☐ |
| `formatDate(new Date(2026,2,31), "en")` | `"03/31/2026"` | ☐ |
| `formatCurrency(1234.56, "fr", "EUR")` | `€` after number | ☐ |
| `formatCurrency(1234.56, "en", "USD")` | `$` before number | ☐ |
| `getLocaleDirection("fr")` | `"ltr"` | ☐ |
| `getLocaleDirection("ar")` | `"rtl"` | ☐ |
| `formatNumber(NaN, "fr")` | `"—"` (em dash, not empty) | ☐ |

**Verification command:**
```powershell
pnpm test src/utils/__tests__/formatters.test.ts
```
Expected: 37 tests pass. Zero failures.

---

## Section 6 — Fallback Chain Behavior

| Check | Expected | Status |
|-------|----------|--------|
| Key exists in fr → shows fr string | ✓ | ☐ |
| Key missing in fr, exists in en → shows en string | ✓ | ☐ |
| Key missing in BOTH locales → shows `[key]`, not empty string | ✓ | ☐ |
| No key shows empty string anywhere in app | ✓ | ☐ |

**Verification command:**
```powershell
pnpm test src/__tests__/i18n/fallback.test.ts
```
Expected: 6 tests pass.

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
```powershell
pnpm run typecheck
```
Expected: No errors. Zero warnings about implicit `any` in i18n-related files.

---

## Section 9 — RTL Infrastructure

| Check | Expected | Status |
|-------|----------|--------|
| `document.documentElement.dir` | `"ltr"` (fr and en) | ☐ |
| `document.documentElement.lang` | `"fr"` (default) | ☐ |
| `LocaleHtmlDir` component mounted in `App.tsx` | ✓ | ☐ |
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

## CI Integration Note

The `pnpm run i18n:check` command must be added to the CI pipeline when the CI
sub-phase is implemented. Until then, developers must run it locally before opening a
PR that touches any file in `src/i18n/`. This is documented in
`docs/TRANSLATION_GOVERNANCE.md` §10.

---

## Sign-off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Developer | | | ☐ |
| Reviewer | | | ☐ |

**SP05 is considered complete when all checklist items are marked ✓ and both
sign-off rows are completed.**

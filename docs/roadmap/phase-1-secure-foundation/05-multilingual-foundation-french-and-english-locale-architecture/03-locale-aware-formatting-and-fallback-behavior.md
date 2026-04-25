# Phase 1 · Sub-phase 05 · File 03
# Locale-Aware Formatting and Fallback Behavior

## Context and Purpose

Files 01 and 02 established the full namespace architecture and wrote all translation
JSON content for the six eager namespaces plus three module starters. What is still
missing is the **runtime formatting layer** — the utilities that convert raw values
(JavaScript `Date`, `number`, `bigint`) into locale-correct display strings.

This is not trivial to skip. A number stored as `1234.56` in the database must render
as `1 234,56` in a French UI and `1,234.56` in an English UI. If developers write
ad-hoc string formatting code directly in components, the locale is not respected, the
formats become inconsistent, and refactoring to add Arabic (right-to-left, Eastern
Arabic numerals) becomes extremely expensive. The formatter utilities built here become
the **single canonical path** for all value rendering in the application.

The file also implements the **fallback chain verification** — tests that prove that
when a French key is missing, the English fallback is shown, and when both are missing,
the visible `[key]` pattern appears. This behavior is assumed to be automatic (it is
configured in `config.ts`), but it must be tested to ensure a future config change does
not silently break the safety net.

Finally, this file sets up the **RTL infrastructure stubs** — a `dir` attribute hook
and a locale metadata structure — so that adding Arabic in Phase 3 does not require
touching every component that currently assumes `dir="ltr"`.

## Architecture Rules Applied

- **Use `Intl.NumberFormat` and `Intl.DateTimeFormat`** — the native browser
  ECMAScript Internationalization API. Do not introduce a formatting library. Tauri's
  WebView embeds Chromium which supports the full Intl API.
- **Formatters are pure functions** — they take a value, a locale string, and options,
  and return a string. They have no side effects and do not read from Zustand or
  i18next internally. This makes them unit-testable with zero setup.
- **The `useFormatters()` hook** binds the current locale from the Zustand locale
  store to the pure formatter functions, returning a stable object of convenience
  methods. Components MUST use the hook, not the raw functions, so that locale switches
  automatically re-render formatted values.
- **French number format:** `1 234,56` — thousands separator is U+202F (NARROW NO-BREAK
  SPACE), decimal separator is comma. `Intl.NumberFormat("fr-FR")` produces this
  correctly.
- **English number format:** `1,234.56` — `Intl.NumberFormat("en-US")`.
- **Currency display:** French: `1 234,56 €` (amount then symbol, narrow-space before
  €). English: `$1,234.56` (symbol then amount, no space).
- **Date format:** French DD/MM/YYYY. English MM/DD/YYYY. Both use `Intl.DateTimeFormat`
  with explicit `{ day, month, year }` options to avoid locale-dependent short format
  differences across OS.
- **Missing key behavior** is NOT changed in this file. `parseMissingKeyHandler`
  in `config.ts` already returns `[ns:key]`. The tests in this file verify that
  behavior end-to-end.
- **RTL stub:** the locale store gains a `direction` derived field (`"ltr"` for fr/en,
  `"rtl"` for any future Arabic locale). The root `<html>` element's `dir` attribute
  is set reactively. Arabic itself is not implemented here.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/utils/formatters.ts` | Pure formatter functions (date, number, currency, relative) |
| `src/hooks/use-formatters.ts` | Hook binding current locale to formatter functions |
| `src/stores/locale-store.ts` (patch) | Add `direction` field and `getDirection()` helper |
| `src/components/locale-html-dir.tsx` | Component that sets `document.documentElement.dir` |
| `src/__tests__/utils/formatters.test.ts` | Unit tests for all formatter functions in both locales |
| `src/__tests__/i18n/fallback.test.ts` | Tests verifying fallback chain (fr → en → [key]) |
| `src/i18n/locale-data/{fr,en}/*.json` | Empty `{}` placeholder files for all 24 module namespaces not yet populated (prevents dynamic import 404) |

## Prerequisites

- SP05-F01 complete: locale store, i18n config, namespaces.ts
- SP05-F02 complete: all eager JSON files populated
- Vitest configured (from SP01-F01 engineering baseline)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Pure Formatter Utilities | `formatters.ts`, `use-formatters.ts`, `locale-html-dir.tsx` |
| S2 | Formatter Unit Tests | `formatters.test.ts` — 18 test cases covering both locales |
| S3 | Fallback Chain Tests and Module Namespace Placeholders | `fallback.test.ts`, 21 empty `{}` placeholder JSON files per locale |

---

## Sprint S1 — Pure Formatter Utilities

### AI Agent Prompt

```
You are a TypeScript engineer building locale-aware formatting utilities for a Tauri
desktop application. The application supports two locales: "fr" (French, format fr-FR)
and "en" (English, format en-US). A third locale (Arabic, ar-DZ) will be added in Phase
3 — all code must be extensible to right-to-left without structural changes.

─────────────────────────────────────────────────────────────────────
CREATE src/utils/formatters.ts
─────────────────────────────────────────────────────────────────────
```typescript
/**
 * formatters.ts
 *
 * Pure locale-aware formatting utilities. All functions are stateless and take
 * an explicit `locale` parameter. Components MUST use the `useFormatters()` hook
 * which binds the current locale automatically.
 *
 * Supported locales: "fr" (→ fr-FR), "en" (→ en-US)
 * Phase 3: "ar" (→ ar-DZ) added here without changing the API.
 */

/** Maps our two-letter locale codes to BCP 47 locale strings for Intl APIs. */
const INTL_LOCALE_MAP: Record<string, string> = {
  fr: "fr-FR",
  en: "en-US",
  ar: "ar-DZ",
};

function toIntlLocale(locale: string): string {
  return INTL_LOCALE_MAP[locale] ?? "fr-FR";
}

// ─── Number Formatting ─────────────────────────────────────────────────────

/**
 * Formats a number according to the active locale.
 *
 * French:  1 234,56   (narrow no-break space as thousands, comma as decimal)
 * English: 1,234.56   (comma as thousands, period as decimal)
 */
export function formatNumber(
  value: number,
  locale: string,
  options: Intl.NumberFormatOptions = {}
): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2,
    ...options,
  }).format(value);
}

/**
 * Formats a number as a decimal with a fixed number of decimal places.
 * Useful for quantity fields where precision is always required.
 */
export function formatDecimal(
  value: number,
  locale: string,
  decimalPlaces = 2
): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    minimumFractionDigits: decimalPlaces,
    maximumFractionDigits: decimalPlaces,
  }).format(value);
}

/**
 * Formats a monetary amount.
 *
 * French:  1 234,56 €    (amount + space + symbol, fr-FR convention)
 * English: $1,234.56     (symbol + amount, en-US convention)
 *
 * Uses Intl.NumberFormat with style: "currency" which handles symbol placement
 * automatically per locale.
 */
export function formatCurrency(
  value: number,
  locale: string,
  currency = "EUR"
): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

/**
 * Formats a number as a percentage.
 * French:  45 %
 * English: 45%
 */
export function formatPercent(value: number, locale: string): string {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    style: "percent",
    minimumFractionDigits: 0,
    maximumFractionDigits: 1,
  }).format(value / 100);
}

// ─── Date Formatting ───────────────────────────────────────────────────────

/**
 * Formats a Date (or ISO string / timestamp) as a short date.
 *
 * French:  31/03/2026
 * English: 03/31/2026
 *
 * Uses explicit day/month/year options to avoid OS-level locale format
 * interpretation differences.
 */
export function formatDate(
  value: Date | string | number | null | undefined,
  locale: string
): string {
  if (value == null) return "—";
  const date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(date);
}

/**
 * Formats a Date as a long human-readable date.
 *
 * French:  31 mars 2026
 * English: March 31, 2026
 */
export function formatDateLong(
  value: Date | string | number | null | undefined,
  locale: string
): string {
  if (value == null) return "—";
  const date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(date);
}

/**
 * Formats a Date including time.
 *
 * French:  31/03/2026 14:30
 * English: 03/31/2026, 2:30 PM
 */
export function formatDateTime(
  value: Date | string | number | null | undefined,
  locale: string
): string {
  if (value == null) return "—";
  const date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: locale === "en",
  }).format(date);
}

/**
 * Formats a time-only value.
 *
 * French:  14:30
 * English: 2:30 PM
 */
export function formatTime(
  value: Date | string | number | null | undefined,
  locale: string
): string {
  if (value == null) return "—";
  const date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    hour: "2-digit",
    minute: "2-digit",
    hour12: locale === "en",
  }).format(date);
}

/**
 * Formats a relative time (e.g., "2 minutes ago", "il y a 2 minutes").
 * Uses Intl.RelativeTimeFormat. Falls back to absolute date for values
 * older than 7 days.
 */
export function formatRelativeTime(
  value: Date | string | number | null | undefined,
  locale: string
): string {
  if (value == null) return "—";
  const date = value instanceof Date ? value : new Date(value);
  if (isNaN(date.getTime())) return "—";

  const now = Date.now();
  const diffMs = date.getTime() - now;
  const diffSeconds = Math.round(diffMs / 1000);
  const absSeconds = Math.abs(diffSeconds);

  const rtf = new Intl.RelativeTimeFormat(toIntlLocale(locale), {
    numeric: "auto",
  });

  if (absSeconds < 60)  return rtf.format(diffSeconds, "second");
  if (absSeconds < 3600) return rtf.format(Math.round(diffSeconds / 60), "minute");
  if (absSeconds < 86400) return rtf.format(Math.round(diffSeconds / 3600), "hour");
  if (absSeconds < 604800) return rtf.format(Math.round(diffSeconds / 86400), "day");

  // Older than 7 days → absolute date
  return formatDate(date, locale);
}

// ─── Text Direction (RTL readiness) ────────────────────────────────────────

/** Returns the text direction for a given locale. */
export function getLocaleDirection(locale: string): "ltr" | "rtl" {
  const RTL_LOCALES = new Set(["ar", "he", "fa", "ur"]);
  return RTL_LOCALES.has(locale) ? "rtl" : "ltr";
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/hooks/use-formatters.ts
─────────────────────────────────────────────────────────────────────
```typescript
/**
 * use-formatters.ts
 *
 * React hook that binds the current active locale (from the locale store)
 * to all pure formatter functions. Components MUST use this hook — they
 * must NOT import and call the raw formatters with a hardcoded locale string.
 *
 * When the user switches locale, all components using this hook automatically
 * re-render with correctly formatted values.
 *
 * Usage:
 *   const { formatDate, formatNumber, formatCurrency } = useFormatters();
 *   return <td>{formatNumber(equipment.costPerHour)}</td>;
 */

import { useMemo } from "react";
import { useLocaleStore } from "../stores/locale-store";
import {
  formatDate,
  formatDateLong,
  formatDateTime,
  formatTime,
  formatRelativeTime,
  formatNumber,
  formatDecimal,
  formatCurrency,
  formatPercent,
} from "../utils/formatters";

export interface Formatters {
  /** "31/03/2026" (fr) or "03/31/2026" (en) */
  formatDate: (value: Date | string | number | null | undefined) => string;
  /** "31 mars 2026" (fr) or "March 31, 2026" (en) */
  formatDateLong: (value: Date | string | number | null | undefined) => string;
  /** "31/03/2026 14:30" (fr) or "03/31/2026, 2:30 PM" (en) */
  formatDateTime: (value: Date | string | number | null | undefined) => string;
  /** "14:30" (fr) or "2:30 PM" (en) */
  formatTime: (value: Date | string | number | null | undefined) => string;
  /** "il y a 5 minutes" (fr) or "5 minutes ago" (en) */
  formatRelativeTime: (value: Date | string | number | null | undefined) => string;
  /** "1 234,56" (fr) or "1,234.56" (en) */
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
  /** Fixed decimal places: "1 234,56" (fr) */
  formatDecimal: (value: number, decimalPlaces?: number) => string;
  /** "1 234,56 €" (fr) or "$1,234.56" (en) */
  formatCurrency: (value: number, currency?: string) => string;
  /** "45,5 %" (fr) or "45.5%" (en) */
  formatPercent: (value: number) => string;
  /** Current locale code ("fr" or "en") */
  locale: string;
}

export function useFormatters(): Formatters {
  const locale = useLocaleStore((s) => s.activeLocale);

  return useMemo(
    () => ({
      formatDate:         (v) => formatDate(v, locale),
      formatDateLong:     (v) => formatDateLong(v, locale),
      formatDateTime:     (v) => formatDateTime(v, locale),
      formatTime:         (v) => formatTime(v, locale),
      formatRelativeTime: (v) => formatRelativeTime(v, locale),
      formatNumber:       (v, opts) => formatNumber(v, locale, opts),
      formatDecimal:      (v, dp) => formatDecimal(v, locale, dp),
      formatCurrency:     (v, cur) => formatCurrency(v, locale, cur),
      formatPercent:      (v) => formatPercent(v, locale),
      locale,
    }),
    [locale]
  );
}
```

─────────────────────────────────────────────────────────────────────
PATCH src/stores/locale-store.ts
─────────────────────────────────────────────────────────────────────
Add a `direction` derived field to the existing locale store. Open
`src/stores/locale-store.ts` and make the following additions:

1. Import `getLocaleDirection` from `../utils/formatters`.
2. Add `direction: "ltr" | "rtl"` to the store state interface.
3. In the `initialize()` function, after setting `activeLocale`, also set:
   `direction: getLocaleDirection(resolved.active_locale)`
4. In the `setLocale()` function, after setting `activeLocale`, also set:
   `direction: getLocaleDirection(locale)`
5. In the store's `create()` call, add the initial value:
   `direction: "ltr"`

The store interface addition (TypeScript):
```typescript
direction: "ltr" | "rtl";
```

─────────────────────────────────────────────────────────────────────
CREATE src/components/locale-html-dir.tsx
─────────────────────────────────────────────────────────────────────
```typescript
/**
 * locale-html-dir.tsx
 *
 * Sets the `dir` attribute on <html> whenever the active locale changes.
 * Renders nothing — mount this once at the root of the application tree.
 *
 * Usage in App.tsx:
 *   <LocaleHtmlDir />     (before any other providers)
 *
 * When Arabic is added (Phase 3), this component will set dir="rtl"
 * automatically without any further changes required.
 */

import { useEffect } from "react";
import { useLocaleStore } from "../stores/locale-store";

export function LocaleHtmlDir(): null {
  const direction = useLocaleStore((s) => s.direction);

  useEffect(() => {
    document.documentElement.setAttribute("dir", direction);
    document.documentElement.setAttribute("lang", useLocaleStore.getState().activeLocale);
  }, [direction]);

  return null;
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` — 0 errors
- `pnpm run dev` — `document.documentElement.dir` is "ltr" (verify in DevTools)
- Components using `useFormatters().formatNumber(1234.56)` display "1 234,56" in
  French and "1,234.56" in English
- Locale switch triggers re-render of formatted values without page reload
```

---

### Supervisor Verification — Sprint S1

**V1 — TypeScript compiles clean.**
Run `pnpm run typecheck`. The formatter functions must fully type-check. In particular,
`useFormatters()` must return the `Formatters` interface with all methods. Each method
that takes options must accept the correct option type.

**V2 — HTML dir attribute is set.**
Open `pnpm run dev`, open DevTools → Elements. The `<html>` element should have
`dir="ltr"` and `lang="fr"` (or `lang="en"` if English is active). If the attribute is
absent, the `LocaleHtmlDir` component is not mounted at the root of `App.tsx`.

**V3 — Manual formatting verification.**
In DevTools console:
```javascript
// Test French formatting
new Intl.NumberFormat("fr-FR", { minimumFractionDigits: 2 }).format(1234.56)
// Expected: "1 234,56" (narrow no-break space before 34)

// Test English formatting
new Intl.NumberFormat("en-US", { minimumFractionDigits: 2 }).format(1234.56)
// Expected: "1,234.56"

// Test French currency
new Intl.NumberFormat("fr-FR", { style: "currency", currency: "EUR" }).format(1234.56)
// Expected: "1 234,56 €"
```
These native calls confirm the WebView supports the Intl API correctly before the
formatters are used in components.

---

## Sprint S2 — Formatter Unit Tests

### AI Agent Prompt

```
You are a TypeScript testing engineer using Vitest. Write comprehensive unit tests for
all formatter functions in `src/utils/formatters.ts`. Each test must cover both the
"fr" and "en" locales. Tests must NOT mock Intl — use the real browser Intl
implementation (Vitest's jsdom environment includes it).

─────────────────────────────────────────────────────────────────────
CREATE src/__tests__/utils/formatters.test.ts
─────────────────────────────────────────────────────────────────────
```typescript
import { describe, it, expect } from "vitest";
import {
  formatNumber,
  formatDecimal,
  formatCurrency,
  formatPercent,
  formatDate,
  formatDateLong,
  formatDateTime,
  formatTime,
  formatRelativeTime,
  getLocaleDirection,
} from "../../utils/formatters";

// ─── Number formatting ──────────────────────────────────────────────────────

describe("formatNumber()", () => {
  it("formats 1234.56 in French with narrow space and comma decimal", () => {
    const result = formatNumber(1234.56, "fr");
    // Intl fr-FR uses narrow no-break space (U+202F) as thousands separator
    expect(result).toContain(",56");
    expect(result).toContain("1");
    expect(result).toContain("234");
  });

  it("formats 1234.56 in English with comma thousands and period decimal", () => {
    expect(formatNumber(1234.56, "en")).toBe("1,234.56");
  });

  it("formats zero correctly in both locales", () => {
    expect(formatNumber(0, "fr")).toBe("0");
    expect(formatNumber(0, "en")).toBe("0");
  });

  it("returns '—' for NaN", () => {
    expect(formatNumber(NaN, "fr")).toBe("—");
    expect(formatNumber(Infinity, "en")).toBe("—");
  });

  it("respects minimumFractionDigits option", () => {
    const result = formatNumber(42, "fr", { minimumFractionDigits: 2 });
    expect(result).toContain(",00");
  });
});

describe("formatDecimal()", () => {
  it("always shows specified decimal places", () => {
    expect(formatDecimal(3, "en", 2)).toBe("3.00");
    expect(formatDecimal(3, "fr", 2)).toContain("3");
  });
});

// ─── Currency formatting ────────────────────────────────────────────────────

describe("formatCurrency()", () => {
  it("formats EUR in French — amount before symbol", () => {
    const result = formatCurrency(1234.56, "fr", "EUR");
    // fr-FR: "1 234,56 €"
    expect(result).toContain("€");
    expect(result).toContain(",56");
    // Symbol should come after the number in fr-FR
    expect(result.indexOf("1")).toBeLessThan(result.indexOf("€"));
  });

  it("formats USD in English — symbol before amount", () => {
    const result = formatCurrency(1234.56, "en", "USD");
    // en-US: "$1,234.56"
    expect(result).toContain("$");
    expect(result).toContain(",234.56");
    expect(result.indexOf("$")).toBeLessThan(result.indexOf("1"));
  });

  it("returns '—' for NaN", () => {
    expect(formatCurrency(NaN, "fr")).toBe("—");
  });

  it("defaults to EUR when currency not specified", () => {
    const result = formatCurrency(100, "fr");
    expect(result).toContain("€");
  });
});

// ─── Percent formatting ─────────────────────────────────────────────────────

describe("formatPercent()", () => {
  it("formats 45 as a percentage", () => {
    const fr = formatPercent(45, "fr");
    const en = formatPercent(45, "en");
    expect(fr).toContain("45");
    expect(en).toContain("45");
    expect(en).toContain("%");
  });

  it("returns '—' for NaN", () => {
    expect(formatPercent(NaN, "fr")).toBe("—");
  });
});

// ─── Date formatting ────────────────────────────────────────────────────────

describe("formatDate()", () => {
  const testDate = new Date(2026, 2, 31, 14, 30, 0); // 31 March 2026, 14:30

  it("formats in French as DD/MM/YYYY", () => {
    expect(formatDate(testDate, "fr")).toBe("31/03/2026");
  });

  it("formats in English as MM/DD/YYYY", () => {
    expect(formatDate(testDate, "en")).toBe("03/31/2026");
  });

  it("accepts ISO string input", () => {
    expect(formatDate("2026-03-31", "fr")).toBe("31/03/2026");
  });

  it("accepts Unix timestamp input", () => {
    const ts = new Date(2026, 2, 31).getTime();
    const result = formatDate(ts, "fr");
    expect(result).toContain("2026");
  });

  it("returns '—' for null", () => {
    expect(formatDate(null, "fr")).toBe("—");
    expect(formatDate(undefined, "en")).toBe("—");
  });

  it("returns '—' for invalid date string", () => {
    expect(formatDate("not-a-date", "fr")).toBe("—");
  });
});

describe("formatDateLong()", () => {
  const testDate = new Date(2026, 2, 31);

  it("formats in French with full month name", () => {
    const result = formatDateLong(testDate, "fr");
    expect(result).toContain("mars");
    expect(result).toContain("2026");
  });

  it("formats in English with full month name", () => {
    const result = formatDateLong(testDate, "en");
    expect(result).toContain("March");
    expect(result).toContain("2026");
  });
});

describe("formatDateTime()", () => {
  const testDate = new Date(2026, 2, 31, 14, 30, 0);

  it("formats in French — 24h time, DD/MM/YYYY", () => {
    const result = formatDateTime(testDate, "fr");
    expect(result).toContain("31/03/2026");
    expect(result).toContain("14:30");
  });

  it("formats in English — 12h time, MM/DD/YYYY", () => {
    const result = formatDateTime(testDate, "en");
    expect(result).toContain("03/31/2026");
    // In en-US Intl, 14:30 → "2:30 PM"
    expect(result.toLowerCase()).toContain("pm");
  });
});

describe("formatTime()", () => {
  const testDate = new Date(2026, 0, 1, 14, 30, 0);

  it("formats in French as 24h", () => {
    expect(formatTime(testDate, "fr")).toBe("14:30");
  });

  it("formats in English as 12h with AM/PM", () => {
    const result = formatTime(testDate, "en");
    expect(result.toLowerCase()).toContain("pm");
  });
});

// ─── Relative time ──────────────────────────────────────────────────────────

describe("formatRelativeTime()", () => {
  it("returns French relative string for recent dates", () => {
    const twoMinutesAgo = new Date(Date.now() - 2 * 60 * 1000);
    const result = formatRelativeTime(twoMinutesAgo, "fr");
    // Intl.RelativeTimeFormat fr: "il y a 2 minutes"
    expect(result.toLowerCase()).toContain("minute");
  });

  it("returns English relative string for recent dates", () => {
    const fiveMinutesAgo = new Date(Date.now() - 5 * 60 * 1000);
    const result = formatRelativeTime(fiveMinutesAgo, "en");
    expect(result.toLowerCase()).toContain("minute");
  });

  it("falls back to absolute date for dates older than 7 days", () => {
    const tenDaysAgo = new Date(Date.now() - 10 * 24 * 60 * 60 * 1000);
    const result = formatRelativeTime(tenDaysAgo, "fr");
    // Should contain year
    expect(result).toMatch(/\d{4}/);
  });

  it("returns '—' for null", () => {
    expect(formatRelativeTime(null, "fr")).toBe("—");
  });
});

// ─── Text direction ─────────────────────────────────────────────────────────

describe("getLocaleDirection()", () => {
  it("returns ltr for French", () => {
    expect(getLocaleDirection("fr")).toBe("ltr");
  });

  it("returns ltr for English", () => {
    expect(getLocaleDirection("en")).toBe("ltr");
  });

  it("returns rtl for Arabic (Phase 3 readiness)", () => {
    expect(getLocaleDirection("ar")).toBe("rtl");
  });

  it("returns ltr for unknown locale", () => {
    expect(getLocaleDirection("xy")).toBe("ltr");
  });
});
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm test src/__tests__/utils/formatters.test.ts` — all tests pass
- No tests mock Intl — they rely on real jsdom Intl implementation
- formatDate("2026-03-31", "fr") === "31/03/2026" confirmed by test
- formatDate("2026-03-31", "en") === "03/31/2026" confirmed by test
- formatCurrency(1234.56, "fr", "EUR") has € after the number
- formatCurrency(1234.56, "en", "USD") has $ before the number
```

---

### Supervisor Verification — Sprint S2

**V1 — All formatter tests pass.**
Run `pnpm test src/__tests__/utils/formatters.test.ts`.
Expected: 18+ tests, all green. If any Intl test fails due to the jsdom environment not
supporting a specific Intl feature, check `vite.config.ts` for the test environment
setting. The environment must be `jsdom` (not `node`) for Intl to be fully available.
Check `vitest.config.ts` for `environment: 'jsdom'`.

**V2 — Date format test is sensitive to time zone.**
The `formatDate("2026-03-31", "fr")` test may fail if the test runner is in a time zone
that shifts midnight March 31 to March 30 UTC when parsing the ISO string. If this
happens, change the test input from an ISO string to `new Date(2026, 2, 31)` (which
uses the local timezone, matching what Intl.DateTimeFormat will output).

---

## Sprint S3 — Fallback Chain Tests and Module Namespace Placeholders

### AI Agent Prompt

```
You are a TypeScript engineer and CMMS domain expert. Your task has two parts:

PART A — Write i18n fallback chain tests.
PART B — Create empty placeholder JSON files for all 24 module namespaces so that
dynamic imports in `resourcesToBackend` don't 404 when a module that has not yet been
translated is loaded for the first time.

─────────────────────────────────────────────────────────────────────
PART A: CREATE src/__tests__/i18n/fallback.test.ts
─────────────────────────────────────────────────────────────────────
This test uses the real i18next instance (initialized for testing) to verify:
1. A French key that exists → returns French string
2. A French key that is MISSING but exists in English → returns English fallback
3. A key that is MISSING in BOTH fr and en → returns the configured missing key handler
   output: "[ns:key]" (as set by parseMissingKeyHandler in config.ts)

```typescript
import { describe, it, expect, beforeAll } from "vitest";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

// Initialize a test-specific i18next instance without the lazy backend.
// We load only the eager namespaces in-memory for fast unit tests.

const FRENCH_COMMON = {
  "app.name": "Maintafox",
  "action.save": "Enregistrer",
  "action.cancel": "Annuler",
};

const ENGLISH_COMMON = {
  "app.name": "Maintafox",
  "action.save": "Save",
  "action.cancel": "Cancel",
  // Deliberately add a key that only exists in English (for fallback test)
  "action.onlyInEnglish": "Only in English",
};

beforeAll(async () => {
  if (i18n.isInitialized) return;

  await i18n.use(initReactI18next).init({
    lng: "fr",
    fallbackLng: "en",
    ns: ["common"],
    defaultNS: "common",
    resources: {
      fr: { common: FRENCH_COMMON },
      en: { common: ENGLISH_COMMON },
    },
    interpolation: { escapeValue: false },
    parseMissingKeyHandler: (key: string) => `[${key}]`,
    saveMissing: false,
  });
});

describe("i18n fallback chain", () => {
  it("returns French string when key exists in fr", () => {
    expect(i18n.t("action.save", { ns: "common" })).toBe("Enregistrer");
  });

  it("returns English fallback when key is missing in fr but exists in en", () => {
    // "action.onlyInEnglish" is not in the fr resources
    const result = i18n.t("action.onlyInEnglish", { ns: "common" });
    expect(result).toBe("Only in English");
  });

  it("returns [key] pattern when key is missing in BOTH fr and en", () => {
    const result = i18n.t("action.nonExistentKey", { ns: "common" });
    // parseMissingKeyHandler returns "[key]" not an empty string
    expect(result).toContain("nonExistentKey");
    expect(result.startsWith("[")).toBe(true);
  });

  it("returns correct app name in both locales", () => {
    expect(i18n.t("app.name", { ns: "common", lng: "fr" })).toBe("Maintafox");
    expect(i18n.t("app.name", { ns: "common", lng: "en" })).toBe("Maintafox");
  });

  it("returns French string when explicitly requesting fr locale", () => {
    expect(i18n.t("action.cancel", { ns: "common", lng: "fr" })).toBe("Annuler");
  });

  it("returns English string when explicitly requesting en locale", () => {
    expect(i18n.t("action.cancel", { ns: "common", lng: "en" })).toBe("Cancel");
  });
});
```

─────────────────────────────────────────────────────────────────────
PART B: Create empty placeholder JSON files for remaining module namespaces
─────────────────────────────────────────────────────────────────────
The following module namespaces are registered in namespaces.ts but their JSON
files have not been created yet. Create them as empty JSON objects `{}` so that
`resourcesToBackend` can complete a dynamic import without throwing a 404 error
or a "Cannot find module" error.

Create the following files (both fr and en), each containing only `{}`:

  src/i18n/locale-data/fr/org.json
  src/i18n/locale-data/en/org.json
  src/i18n/locale-data/fr/personnel.json
  src/i18n/locale-data/en/personnel.json
  src/i18n/locale-data/fr/reference.json
  src/i18n/locale-data/en/reference.json
  src/i18n/locale-data/fr/inventory.json
  src/i18n/locale-data/en/inventory.json
  src/i18n/locale-data/fr/pm.json
  src/i18n/locale-data/en/pm.json
  src/i18n/locale-data/fr/planning.json
  src/i18n/locale-data/en/planning.json
  src/i18n/locale-data/fr/permits.json
  src/i18n/locale-data/en/permits.json
  src/i18n/locale-data/fr/inspections.json
  src/i18n/locale-data/en/inspections.json
  src/i18n/locale-data/fr/training.json
  src/i18n/locale-data/en/training.json
  src/i18n/locale-data/fr/reliability.json
  src/i18n/locale-data/en/reliability.json
  src/i18n/locale-data/fr/budget.json
  src/i18n/locale-data/en/budget.json
  src/i18n/locale-data/fr/reports.json
  src/i18n/locale-data/en/reports.json
  src/i18n/locale-data/fr/archive.json
  src/i18n/locale-data/en/archive.json
  src/i18n/locale-data/fr/notifications.json
  src/i18n/locale-data/en/notifications.json
  src/i18n/locale-data/fr/documentation.json
  src/i18n/locale-data/en/documentation.json
  src/i18n/locale-data/fr/iot.json
  src/i18n/locale-data/en/iot.json
  src/i18n/locale-data/fr/erp.json
  src/i18n/locale-data/en/erp.json
  src/i18n/locale-data/fr/activity.json
  src/i18n/locale-data/en/activity.json
  src/i18n/locale-data/fr/users.json
  src/i18n/locale-data/en/users.json
  src/i18n/locale-data/fr/settings.json
  src/i18n/locale-data/en/settings.json
  src/i18n/locale-data/fr/configuration.json
  src/i18n/locale-data/en/configuration.json

Note: equipment, di, and ot already have real content from SP05-F02.

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm test src/__tests__/i18n/fallback.test.ts` — all 6 fallback tests pass
- All 42 placeholder files exist and contain valid JSON (`{}`)
- Navigating to any module route in `pnpm run dev` does not trigger a network 404
  or a console error about missing locale-data JSON files
- The missing-key test (key absent from both locales) shows a string starting with
  "[" — NOT an empty string
```

---

### Supervisor Verification — Sprint S3

**V1 — Fallback tests pass.**
Run `pnpm test src/__tests__/i18n/fallback.test.ts`.
All 6 tests must be green. If the missing-key test fails (shows empty string instead of
`[key]`), it means the `parseMissingKeyHandler` in `config.ts` is not being applied to
this test instance. In the test's `beforeAll` init, explicitly add
`parseMissingKeyHandler: (key) => \`[\${key}]\`` to the i18next init options to make
the test self-contained.

**V2 — No 404 on module namespace import.**
In `pnpm run dev`, open DevTools → Console. Navigate between all available module routes.
If any route triggers a console warning like "i18next::backendConnector: loading namespace
'pm' for language 'fr' failed" with a module-not-found error, the corresponding
placeholder file is missing. Run
`Get-ChildItem src/i18n/locale-data/fr/ | Measure-Object` — the count should be 27
(equipment, di, ot + 24 namespaces minus the three already done = 24 total in locale-data;
but since di and ot are real content the count is 27 fr files). Verify:
```
pnpm exec node -e "
const ns = require('./src/i18n/namespaces.ts').MODULE_NAMESPACES;
const fs = require('fs');
ns.forEach(n => {
  const fr = \`src/i18n/locale-data/fr/\${n}.json\`;
  const en = \`src/i18n/locale-data/en/\${n}.json\`;
  if (!fs.existsSync(fr)) console.error('MISSING:', fr);
  if (!fs.existsSync(en)) console.error('MISSING:', en);
});
console.log('check complete');
"
```
(Adjust the require path if TypeScript files need tsx). No MISSING lines should appear.

**V3 — Total namespace count.**
Verify `EAGER_NAMESPACES.length + MODULE_NAMESPACES.length === 30`.
Run: `pnpm exec tsx -e "import { EAGER_NAMESPACES, MODULE_NAMESPACES } from './src/i18n/namespaces'; console.log(EAGER_NAMESPACES.length + MODULE_NAMESPACES.length);"`
Output must be `30`.

---

*End of Phase 1 · Sub-phase 05 · File 03*

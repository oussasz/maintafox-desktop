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

  it("returns em-dash for NaN and Infinity", () => {
    expect(formatNumber(NaN, "fr")).toBe("\u2014");
    expect(formatNumber(Infinity, "en")).toBe("\u2014");
    expect(formatNumber(-Infinity, "fr")).toBe("\u2014");
  });

  it("respects minimumFractionDigits option", () => {
    const result = formatNumber(42, "fr", { minimumFractionDigits: 2 });
    expect(result).toContain(",00");
  });
});

describe("formatDecimal()", () => {
  it("always shows specified decimal places in English", () => {
    expect(formatDecimal(3, "en", 2)).toBe("3.00");
  });

  it("always shows specified decimal places in French", () => {
    const result = formatDecimal(3, "fr", 2);
    expect(result).toContain("3,00");
  });

  it("returns em-dash for NaN", () => {
    expect(formatDecimal(NaN, "en", 2)).toBe("\u2014");
  });
});

// ─── Currency formatting ────────────────────────────────────────────────────

describe("formatCurrency()", () => {
  it("formats EUR in French — amount before symbol", () => {
    const result = formatCurrency(1234.56, "fr", "EUR");
    // fr-FR: "1 234,56 €"
    expect(result).toContain("\u20AC"); // €
    expect(result).toContain(",56");
    // Symbol should come after the number in fr-FR
    expect(result.indexOf("1")).toBeLessThan(result.indexOf("\u20AC"));
  });

  it("formats USD in English — symbol before amount", () => {
    const result = formatCurrency(1234.56, "en", "USD");
    // en-US: "$1,234.56"
    expect(result).toContain("$");
    expect(result).toContain(",234.56");
    expect(result.indexOf("$")).toBeLessThan(result.indexOf("1"));
  });

  it("returns em-dash for NaN", () => {
    expect(formatCurrency(NaN, "fr")).toBe("\u2014");
  });

  it("defaults to EUR when currency not specified", () => {
    const result = formatCurrency(100, "fr");
    expect(result).toContain("\u20AC");
  });
});

// ─── Percent formatting ─────────────────────────────────────────────────────

describe("formatPercent()", () => {
  it("formats 45 as a percentage in both locales", () => {
    const fr = formatPercent(45, "fr");
    const en = formatPercent(45, "en");
    expect(fr).toContain("45");
    expect(fr).toContain("%");
    expect(en).toContain("45");
    expect(en).toContain("%");
  });

  it("returns em-dash for NaN", () => {
    expect(formatPercent(NaN, "fr")).toBe("\u2014");
  });
});

// ─── Date formatting ────────────────────────────────────────────────────────

describe("formatDate()", () => {
  // Use local-timezone Date constructor to avoid UTC midnight drift.
  const testDate = new Date(2026, 2, 31, 14, 30, 0); // 31 March 2026, 14:30

  it("formats in French as DD/MM/YYYY", () => {
    expect(formatDate(testDate, "fr")).toBe("31/03/2026");
  });

  it("formats in English as MM/DD/YYYY", () => {
    expect(formatDate(testDate, "en")).toBe("03/31/2026");
  });

  it("accepts Date constructor from ISO string input", () => {
    // Use local Date constructor to avoid timezone shift with raw ISO strings
    const d = new Date(2026, 2, 31);
    expect(formatDate(d, "fr")).toBe("31/03/2026");
  });

  it("accepts Unix timestamp input", () => {
    const ts = new Date(2026, 2, 31).getTime();
    const result = formatDate(ts, "fr");
    expect(result).toContain("31/03/2026");
  });

  it("returns em-dash for null and undefined", () => {
    expect(formatDate(null, "fr")).toBe("\u2014");
    expect(formatDate(undefined, "en")).toBe("\u2014");
  });

  it("returns em-dash for invalid date string", () => {
    expect(formatDate("not-a-date", "fr")).toBe("\u2014");
  });
});

describe("formatDateLong()", () => {
  const testDate = new Date(2026, 2, 31);

  it("formats in French with full month name", () => {
    const result = formatDateLong(testDate, "fr");
    expect(result).toContain("mars");
    expect(result).toContain("2026");
    expect(result).toContain("31");
  });

  it("formats in English with full month name", () => {
    const result = formatDateLong(testDate, "en");
    expect(result).toContain("March");
    expect(result).toContain("2026");
    expect(result).toContain("31");
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
    expect(result).toContain("2");
    expect(result).toContain("30");
  });

  it("returns em-dash for null", () => {
    expect(formatTime(null, "fr")).toBe("\u2014");
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
    // Should contain year (i.e., 4 digits)
    expect(result).toMatch(/\d{4}/);
  });

  it("returns em-dash for null", () => {
    expect(formatRelativeTime(null, "fr")).toBe("\u2014");
  });

  it("returns em-dash for invalid date", () => {
    expect(formatRelativeTime("not-a-date", "en")).toBe("\u2014");
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

  it("returns rtl for Hebrew", () => {
    expect(getLocaleDirection("he")).toBe("rtl");
  });

  it("returns ltr for unknown locale", () => {
    expect(getLocaleDirection("xy")).toBe("ltr");
  });
});

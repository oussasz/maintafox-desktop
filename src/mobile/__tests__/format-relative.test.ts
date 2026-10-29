import { describe, expect, it } from "vitest";

import { type MobileT, createMobileI18n } from "../i18n";
import { formatRelativeTime } from "../lib/format";

function tOf(language: "fr" | "en" | "ar"): MobileT {
  const i18n = createMobileI18n(language);
  return (key, options) => String(i18n.t(key, options as never));
}

const NOW = Date.parse("2026-10-29T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const SEC = 1000;
const MIN = 60 * SEC;
const HOUR = 60 * MIN;

function french(value: string | null | undefined) {
  return formatRelativeTime(value, NOW, "fr", tOf("fr"));
}

describe("formatRelativeTime", () => {
  it("says 'à l'instant' under a minute, including right now and clock skew", () => {
    expect(french(ago(0))).toBe("à l'instant");
    expect(french(ago(59 * SEC))).toBe("à l'instant");
    expect(french(ago(-30 * SEC))).toBe("à l'instant");
  });

  it("counts minutes from 1 to 59", () => {
    expect(french(ago(MIN))).toBe("il y a 1 min");
    expect(french(ago(59 * MIN + 59 * SEC))).toBe("il y a 59 min");
  });

  it("counts hours from 1 up to a day", () => {
    expect(french(ago(HOUR))).toBe("il y a 1 h");
    expect(french(ago(23 * HOUR + 59 * MIN))).toBe("il y a 23 h");
  });

  it("gives a date from a day on", () => {
    const text = french(ago(24 * HOUR));
    expect(text).toMatch(/^le /);
    expect(text).toContain("2026");
    expect(french(ago(40 * 24 * HOUR))).toMatch(/^le /);
  });

  it("is null when the instant is absent or unreadable", () => {
    expect(french(null)).toBeNull();
    expect(french(undefined)).toBeNull();
    expect(french("not a date")).toBeNull();
  });

  it("is translated in English and Arabic", () => {
    expect(formatRelativeTime(ago(5 * MIN), NOW, "en", tOf("en"))).toBe("5 min ago");
    expect(formatRelativeTime(ago(2 * HOUR), NOW, "ar", tOf("ar"))).toBe("منذ 2 س");
    createMobileI18n("fr");
  });
});

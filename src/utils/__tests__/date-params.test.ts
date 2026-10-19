import { describe, expect, it } from "vitest";

import { localizeDateTimeParams, storedInstant } from "@/utils/date-params";
import { formatDateTime } from "@/utils/formatters";

describe("storedInstant", () => {
  it("reads RFC 3339 times, and a time without a zone as UTC (as the backend does)", () => {
    expect(storedInstant("2026-10-19T06:30:00Z")?.toISOString()).toBe("2026-10-19T06:30:00.000Z");
    expect(storedInstant("2026-10-19T08:30:00+02:00")?.toISOString()).toBe(
      "2026-10-19T06:30:00.000Z",
    );
    expect(storedInstant(" 2026-10-19 06:30:00 ")?.toISOString()).toBe("2026-10-19T06:30:00.000Z");
  });

  it("gives nothing for an unreadable or empty value", () => {
    expect(storedInstant("someday")).toBeNull();
    expect(storedInstant("  ")).toBeNull();
  });
});

describe("localizeDateTimeParams", () => {
  const instant = new Date(Date.UTC(2026, 9, 19, 6, 30));

  it("shows the time params as local date and time in the user's language", () => {
    for (const locale of ["fr", "en"]) {
      const out = localizeDateTimeParams(
        {
          permit: "PTW-0001",
          expires_at: "2026-10-19T06:30:00Z",
          planned_end: "2026-10-19 06:30:00",
          last: "2026-10-19T06:30:00.000Z",
        },
        locale,
      );
      expect(out).toEqual({
        permit: "PTW-0001",
        expires_at: formatDateTime(instant, locale),
        planned_end: formatDateTime(instant, locale),
        last: formatDateTime(instant, locale),
      });
    }
    expect(formatDateTime(instant, "fr")).not.toBe(formatDateTime(instant, "en"));
  });

  it("shows a number of hours with the language's decimal separator", () => {
    expect(localizeDateTimeParams({ hours: "2.5" }, "fr")).toEqual({ hours: "2,5" });
    expect(localizeDateTimeParams({ hours: "2.5" }, "en")).toEqual({ hours: "2.5" });
    expect(localizeDateTimeParams({ hours: "0.33" }, "fr")).toEqual({ hours: "0,33" });
    expect(localizeDateTimeParams({ hours: "48" }, "fr")).toEqual({ hours: "48" });
    // Not a plain number: kept as sent.
    expect(localizeDateTimeParams({ hours: "inf" }, "fr")).toEqual({ hours: "inf" });
  });

  it("keeps an unreadable time as sent, and leaves other params alone", () => {
    expect(
      localizeDateTimeParams({ planned_end: "someday", date: "2026-10-19", code: "X" }, "fr"),
    ).toEqual({ planned_end: "someday", date: "2026-10-19", code: "X" });
  });
});

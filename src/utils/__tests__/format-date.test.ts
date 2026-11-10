import { describe, expect, it } from "vitest";

import { formatDate, formatDateTime, formatShortDate } from "../format-date";

describe("format-date helpers", () => {
  it("shows a calendar day as dd/mm/yyyy, never shifted by the time zone", () => {
    expect(formatDate("2026-10-05", "fr")).toBe("05/10/2026");
    expect(formatDate("2026-10-05", "en")).toBe("10/05/2026");
  });

  it("shows a date and time as dd/mm/yyyy hh:mm in local time", () => {
    expect(formatDateTime("2026-10-31T22:00", "fr")).toMatch(/^31\/10\/2026.*22:00$/);
    expect(formatDateTime("2026-10-31 21:00:00", "fr")).toBe(
      formatDateTime("2026-10-31T21:00:00.000Z", "fr"),
    );
  });

  it("shows a short day as dd/mm", () => {
    expect(formatShortDate("2026-10-31", "fr")).toBe("31/10");
  });

  it("shows a dash for an empty or unreadable value", () => {
    expect(formatDate(null, "fr")).toBe("—");
    expect(formatDateTime("", "fr")).toBe("—");
    expect(formatDate("not a date", "fr")).toBe("—");
  });
});

import { describe, expect, it } from "vitest";

import {
  dateInputFormat,
  dateInputPlaceholder,
  formatIsoDateForInput,
  isDateWithin,
  isIsoDate,
  joinDateTime,
  parseDateInput,
  parseTimeInput,
  splitDateTime,
} from "../date-input";

describe("date input text", () => {
  it("writes dates day first in French and month first in English", () => {
    expect(dateInputFormat("fr").order).toEqual(["day", "month", "year"]);
    expect(dateInputFormat("en").order).toEqual(["month", "day", "year"]);
    expect(formatIsoDateForInput("2026-10-25", "fr")).toBe("25/10/2026");
    expect(formatIsoDateForInput("2026-10-25", "en")).toBe("10/25/2026");
    expect(formatIsoDateForInput("", "fr")).toBe("");
    expect(formatIsoDateForInput("2026-02-31", "fr")).toBe("");
    expect(dateInputPlaceholder("fr")).toBe("jj/mm/aaaa");
    expect(dateInputPlaceholder("en")).toBe("mm/dd/yyyy");
  });

  it("reads typed dates back to ISO", () => {
    expect(parseDateInput("25/10/2026", "fr")).toBe("2026-10-25");
    expect(parseDateInput("5.1.26", "fr")).toBe("2026-01-05");
    expect(parseDateInput("25-10-2026", "fr")).toBe("2026-10-25");
    expect(parseDateInput("10/25/2026", "en")).toBe("2026-10-25");
    expect(parseDateInput("2026-10-25", "fr")).toBe("2026-10-25");
  });

  it("refuses text that is not a real date", () => {
    for (const text of ["", "31/02/2026", "25/13/2026", "25/10", "ab/cd/efgh", "25/10/202"]) {
      expect(parseDateInput(text, "fr")).toBeNull();
    }
  });

  it("checks bounds and ISO validity", () => {
    expect(isIsoDate("2024-02-29")).toBe(true);
    expect(isIsoDate("2025-02-29")).toBe(false);
    expect(isDateWithin("2026-10-25", "2026-10-01", "2026-10-31")).toBe(true);
    expect(isDateWithin("2026-11-01", undefined, "2026-10-31")).toBe(false);
    expect(isDateWithin("2026-09-30", "2026-10-01")).toBe(false);
  });
});

describe("24-hour time text", () => {
  it("reads common ways of typing a time", () => {
    expect(parseTimeInput("08:05")).toBe("08:05");
    expect(parseTimeInput("8:05")).toBe("08:05");
    expect(parseTimeInput("0805")).toBe("08:05");
    expect(parseTimeInput("805")).toBe("08:05");
    expect(parseTimeInput("8h05")).toBe("08:05");
    expect(parseTimeInput("23:59")).toBe("23:59");
  });

  it("refuses times that do not exist", () => {
    for (const text of ["", "24:00", "12:60", "2560", "ab:cd", "8:5"]) {
      expect(parseTimeInput(text)).toBeNull();
    }
  });

  it("splits and joins date-time values", () => {
    expect(splitDateTime("2026-10-25T08:30")).toEqual({ date: "2026-10-25", time: "08:30" });
    expect(splitDateTime("")).toEqual({ date: "", time: "" });
    expect(joinDateTime("2026-10-25", "08:30")).toBe("2026-10-25T08:30");
    expect(joinDateTime("2026-10-25", "")).toBe("");
  });
});

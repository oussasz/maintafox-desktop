import { describe, expect, it } from "vitest";

/**
 * Native date, datetime-local and time inputs follow the Windows region (mm/dd/yyyy, AM/PM in a
 * French app) and clip their calendar icon in narrow panels. Screens use DateField,
 * DateTimeField, DateTimeLocalField and TimeInput instead.
 */
const sources = import.meta.glob("/src/**/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
});

const NATIVE = /type=(?:"|\{\s*")(date|datetime-local|time|month|week)(?:"|\s*"\s*\})/;

describe("no native date inputs", () => {
  it("finds no native date or time input in any screen", () => {
    const offenders = Object.entries(sources)
      .filter(([path]) => !path.includes("/__tests__/"))
      .filter(([, text]) => NATIVE.test(String(text)))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });
});

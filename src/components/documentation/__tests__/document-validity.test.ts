import { describe, expect, it } from "vitest";

import { EXPIRY_WARNING_DAYS, documentValidity, localIsoDate } from "../document-validity";

// 15 March 2027, local time.
const NOW = new Date(2027, 2, 15, 10, 30);

describe("documentValidity", () => {
  it("is null for a document that never expires", () => {
    expect(documentValidity(null, NOW)).toBeNull();
    expect(documentValidity("", NOW)).toBeNull();
  });

  it("is expired from the day after the last valid day", () => {
    expect(documentValidity("2027-03-14", NOW)).toBe("expired");
    expect(documentValidity("2027-03-15", NOW)).toBe("soon");
  });

  it(`warns ${EXPIRY_WARNING_DAYS} days ahead, across a month end`, () => {
    expect(documentValidity("2027-04-14", NOW)).toBe("soon");
    expect(documentValidity("2027-04-15", NOW)).toBe("valid");
  });

  it("uses the local calendar date", () => {
    expect(localIsoDate(new Date(2027, 0, 5, 23, 59))).toBe("2027-01-05");
  });
});

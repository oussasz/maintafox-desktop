import { describe, expect, it, vi } from "vitest";
import { z } from "zod";

import { i18n } from "@/i18n/config";
import { appErrorReason, describeAppError } from "@/lib/app-error";

describe("describeAppError", () => {
  it("shows a plain message, not the raw schema dump, when a response has an unexpected format", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const result = z
      .object({ shift: z.enum(["morning", "afternoon", "night", "full_day"]).nullable() })
      .safeParse({ shift: "jour" });
    if (result.success) throw new Error("expected a schema failure");

    const described = describeAppError(result.error);

    expect(described.code).toBe("INVALID_RESPONSE");
    expect(described.title).toBe(String(i18n.t("errors:generic.invalidData")));
    expect(described.title).not.toBe("errors:generic.invalidData");
    for (const text of [described.title, described.description ?? ""]) {
      expect(text).not.toContain("invalid_enum_value");
      expect(text).not.toContain("jour");
      expect(text).not.toContain("[{");
    }
    // The technical detail is kept for diagnosis, in the console only.
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });
});

describe("appErrorReason", () => {
  it("gives the coded rule rather than a generic validation title", () => {
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: Two-person rule",
      details: [{ code: "PERMIT_TWO_PERSON_VERIFY", message: "Two-person rule", params: {} }],
    };
    const reason = appErrorReason(err);
    expect(reason).not.toBe(describeAppError(err).title);
    expect(reason.length).toBeGreaterThan(0);
  });

  it("falls back to the title for other failures", () => {
    const err = { code: "DATABASE_ERROR", message: "disk I/O error" };
    expect(appErrorReason(err)).toBe(describeAppError(err).title);
  });
});

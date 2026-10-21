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

  it("translates a refusal sent as a bare rule code instead of showing the code", async () => {
    await i18n.loadNamespaces("reliability");
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: DECISION_SELF_APPROVAL",
      details: [{ message: "DECISION_SELF_APPROVAL" }],
    };
    const reason = appErrorReason(err);
    expect(reason).toBe(String(i18n.t("reliability:errors.DECISION_SELF_APPROVAL")));
    expect(reason).not.toContain("DECISION_SELF_APPROVAL");
  });

  it("keeps a plain sentence as it is (not mistaken for a code)", () => {
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: Title is required",
      details: [{ message: "Title is required" }],
    };
    expect(appErrorReason(err)).toBe("Title is required");
  });

  it("falls back to the title for other failures", () => {
    const err = { code: "DATABASE_ERROR", message: "disk I/O error" };
    expect(appErrorReason(err)).toBe(describeAppError(err).title);
  });
});

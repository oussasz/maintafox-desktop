import { describe, expect, it, vi } from "vitest";
import { z } from "zod";

import { i18n } from "@/i18n/config";
import { appErrorLines, appErrorReason, describeAppError } from "@/lib/app-error";

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

  it("names the frozen week when a reassignment would move the job into it", async () => {
    await i18n.loadNamespaces("training");
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: The week of 2026-05-04 is frozen",
      details: [
        {
          code: "PLANNING_WEEK_FROZEN",
          message: "The week of 2026-05-04 is frozen",
          params: { week_start: "2026-05-04" },
        },
      ],
    };
    const reason = appErrorReason(err);
    expect(reason).not.toBe(describeAppError(err).title);
    expect(reason).not.toContain("PLANNING_WEEK_FROZEN");
    expect(reason).toBe(
      String(i18n.t("training:errors.PLANNING_WEEK_FROZEN", { week_start: "2026-05-04" })),
    );
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

describe("one path for every refusal", () => {
  it("translates a coded refusal on a cold start, with no screen having opened its module", async () => {
    const { initI18n, issueNamespacesReady } = await import("@/i18n");
    initI18n();
    await i18n.changeLanguage("fr");
    await issueNamespacesReady();
    // Planning/Work-order refusals are texts of the training namespace, never opened here.
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: PLANNING_WEEK_FROZEN",
      details: [{ code: "PLANNING_WEEK_FROZEN", message: "The week is frozen", params: {} }],
    };
    const reason = appErrorReason(err);
    expect(reason).not.toContain("frozen");
    expect(reason).not.toContain("PLANNING_WEEK_FROZEN");
    expect(reason).toBe(String(i18n.t("training:errors.PLANNING_WEEK_FROZEN")));
  });

  it("never shows a GATE code, and keeps the raw text for the technical disclosure", async () => {
    await i18n.changeLanguage("fr");
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_SOMETHING_NEW: internal sentence in English",
    };
    const described = describeAppError(err);
    expect(described.title).toBe(String(i18n.t("errors:appError.VALIDATION_FAILED")));
    expect(described.title).not.toContain("GATE_");
    expect(described.description ?? "").not.toContain("GATE_");
    expect(described.technical).toContain("GATE_SOMETHING_NEW");
  });

  it("shows a translated gate code in French", async () => {
    await i18n.changeLanguage("fr");
    const described = describeAppError({
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_SLA_RULE_NEEDS_SCOPE: règle SLA sans portée",
    });
    expect(described.title).toBe(String(i18n.t("errors:gateError.GATE_SLA_RULE_NEEDS_SCOPE")));
    expect(described.title).not.toContain("GATE_");
  });
});

describe("appErrorLines", () => {
  it("gives each reason of a refusal as its own translated line", async () => {
    await i18n.changeLanguage("fr");
    const err = {
      code: "VALIDATION_FAILED",
      message: "Validation failed: two reasons",
      details: [
        {
          message:
            "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED:min_len=10 (reason required)",
        },
        { code: "PLANNING_WEEK_FROZEN", message: "The week is frozen" },
      ],
    };
    const lines = appErrorLines(err);
    expect(lines).toEqual([
      String(i18n.t("errors:gateError.GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED")),
      String(i18n.t("training:errors.PLANNING_WEEK_FROZEN")),
    ]);
    for (const line of lines) expect(line).not.toContain("GATE_");
  });

  it("is one line when the refusal has no details", async () => {
    await i18n.changeLanguage("fr");
    expect(appErrorLines({ code: "PERMISSION_DENIED", message: "Permission denied" })).toEqual([
      String(i18n.t("errors:appError.PERMISSION_DENIED")),
    ]);
  });

  it("never shows a GATE code, even one with no translation", async () => {
    await i18n.changeLanguage("fr");
    const lines = appErrorLines({
      code: "VALIDATION_FAILED",
      message: "Validation failed: x",
      details: [{ message: "GATE_NOT_YET_TRANSLATED:min_len=3 (reason)" }],
    });
    expect(lines).toEqual([String(i18n.t("errors:generic.unexpected"))]);
  });

  it("keeps the backend sentence of an uncoded detail (prefix removed) and drops empty ones", async () => {
    await i18n.changeLanguage("fr");
    const lines = appErrorLines({
      code: "VALIDATION_FAILED",
      message: "Validation failed: a; b",
      details: [{ message: "Validation failed: Notes obligatoires" }, { message: "   " }],
    });
    expect(lines).toEqual(["Notes obligatoires"]);
  });
});

describe("details that carry their own code and parameters", () => {
  it("translates a GATE detail from its code and fills its parameters", async () => {
    await i18n.changeLanguage("fr");
    const lines = appErrorLines({
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED:min_len=10",
      details: [
        {
          code: "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED",
          params: { min_len: "10" },
          message: "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED:min_len=10",
        },
      ],
    });
    expect(lines).toEqual([
      String(i18n.t("errors:gateError.GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED")),
    ]);
  });

  it("never shows the code of a GATE detail that has no text", async () => {
    await i18n.changeLanguage("fr");
    const lines = appErrorLines({
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_NO_TEXT_YET: sentence",
      details: [{ code: "GATE_NO_TEXT_YET", params: {}, message: "GATE_NO_TEXT_YET: sentence" }],
    });
    expect(lines).toEqual([String(i18n.t("errors:generic.unexpected"))]);
  });
});

describe("a GATE refusal's title uses the parameters the backend sent", () => {
  it("fills the from and to of a status change", async () => {
    await i18n.changeLanguage("fr");
    const described = describeAppError({
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_FIN_VERSION_TRANSITION:from=draft,to=closed: Invalid",
      details: [
        {
          code: "GATE_FIN_VERSION_TRANSITION",
          params: { from: "draft", to: "closed" },
          message: "GATE_FIN_VERSION_TRANSITION:from=draft,to=closed: Invalid",
        },
      ],
    });
    expect(described.title).toContain("Brouillon");
    expect(described.title).toContain("Clôturée");
    expect(described.title).not.toContain("{{");
  });
});

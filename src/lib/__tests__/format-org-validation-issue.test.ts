import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { i18n, initI18n } from "@/i18n";
import {
  formatOrgIssueWithI18n,
  formatOrgValidationIssue,
} from "@/lib/format-org-validation-issue";
import { formatDateTime } from "@/utils/formatters";

const dictionary: Record<string, string> = {
  "errors.ORG_NODE_LOCKED": "Nœud verrouillé",
  "equipment:errors.ASSET_CODE_TAKEN": "Le code {{code}} est déjà utilisé",
  "reference:errors.REF_VALUE_IN_USE": "Valeur utilisée",
};

const t = ((key: string, opts: Record<string, unknown>) => {
  const template = dictionary[key];
  if (template === undefined) return opts["defaultValue"] ?? key;
  return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts[name] ?? ""));
}) as unknown as Parameters<typeof formatOrgValidationIssue>[1];

describe("formatOrgValidationIssue", () => {
  it("translates org codes from the org namespace", () => {
    expect(formatOrgValidationIssue({ code: "ORG_NODE_LOCKED", message: "Node locked" }, t)).toBe(
      "Nœud verrouillé",
    );
  });

  it("translates asset and reference codes from their own namespaces, with params", () => {
    expect(
      formatOrgValidationIssue(
        { code: "ASSET_CODE_TAKEN", message: "Code taken", params: { code: "P-101" } },
        t,
      ),
    ).toBe("Le code P-101 est déjà utilisé");
    expect(formatOrgValidationIssue({ code: "REF_VALUE_IN_USE", message: "In use" }, t)).toBe(
      "Valeur utilisée",
    );
  });

  it("falls back to the backend message for unknown codes", () => {
    expect(formatOrgValidationIssue({ code: "UNKNOWN", message: "Backend text" }, t)).toBe(
      "Backend text",
    );
  });
});

describe("coded refusals with times", () => {
  let before = "";
  beforeAll(() => {
    initI18n();
    before = i18n.language;
  });
  afterAll(async () => {
    await i18n.changeLanguage(before);
  });

  // PERMIT_EXPIRES_BEFORE_PLANNED_END carries the stored UTC times; the user reads local times.
  const issue = {
    code: "PERMIT_EXPIRES_BEFORE_PLANNED_END",
    message: "Permit PTW-0001 expires (2026-10-19T06:30:00Z) before ...",
    params: {
      permit: "PTW-0001",
      expires_at: "2026-10-19T06:30:00Z",
      planned_end: "2026-10-20 14:00:00",
    },
  };

  it.each(["fr", "en"])("shows them as local date and time in %s", async (language) => {
    await i18n.changeLanguage(language);
    await i18n.loadNamespaces("training");
    const text = formatOrgIssueWithI18n(issue);
    expect(text).toContain("PTW-0001");
    expect(text).toContain(formatDateTime(new Date(Date.UTC(2026, 9, 19, 6, 30)), language));
    expect(text).toContain(formatDateTime(new Date(Date.UTC(2026, 9, 20, 14, 0)), language));
    expect(text).not.toContain("2026-10-19T06:30:00Z");
    expect(text).not.toContain("2026-10-20 14:00:00");
    expect(text).toBe(
      String(
        i18n.t("training:errors.PERMIT_EXPIRES_BEFORE_PLANNED_END", {
          permit: "PTW-0001",
          expires_at: formatDateTime(new Date(Date.UTC(2026, 9, 19, 6, 30)), language),
          planned_end: formatDateTime(new Date(Date.UTC(2026, 9, 20, 14, 0)), language),
        }),
      ),
    );
  });

  // Without a planned end the refusal names the estimated end (now + the expected duration).
  it.each(["fr", "en"])("names the estimated end and the duration in %s", async (language) => {
    await i18n.changeLanguage(language);
    await i18n.loadNamespaces("training");
    const text = formatOrgIssueWithI18n({
      code: "PERMIT_EXPIRES_BEFORE_ESTIMATED_END",
      message: "Permit PTW-0002 expires ...",
      params: {
        permit: "PTW-0002",
        expires_at: "2026-10-19T06:30:00Z",
        end_at: "2026-10-19T09:00:00Z",
        hours: "2.5",
      },
    });
    expect(text).not.toBe("Permit PTW-0002 expires ...");
    expect(text).toContain("PTW-0002");
    // The hours in the user's language (review NOTE 4): a decimal comma in French.
    expect(text).toContain(language === "fr" ? "2,5 h" : "2.5 h");
    expect(text).toContain(formatDateTime(new Date(Date.UTC(2026, 9, 19, 9, 0)), language));
    expect(text).not.toContain("2026-10-19T09:00:00Z");
  });

  // Review SF1: a planned end already past with no duration asks to re-plan or enter a duration.
  it.each(["fr", "en"])("explains a planned end already past in %s", async (language) => {
    await i18n.changeLanguage(language);
    await i18n.loadNamespaces("training");
    const params = {
      permit: "PTW-0003",
      expires_at: "2026-10-19T08:05:00Z",
      planned_end: "2026-10-18T11:00:00Z",
    };
    const text = formatOrgIssueWithI18n({
      code: "WO_PLANNED_END_PASSED",
      message: "The work order's planned end ...",
      params,
    });
    expect(text).not.toBe("The work order's planned end ...");
    expect(text).toContain("PTW-0003");
    expect(text).toContain(formatDateTime(new Date(Date.UTC(2026, 9, 18, 11, 0)), language));
    expect(text).not.toContain("2026-10-18T11:00:00Z");
  });

  it.each(["fr", "en"])(
    "names an unreadable duration and an invalid remaining time in %s",
    async (language) => {
      await i18n.changeLanguage(language);
      await i18n.loadNamespaces("training");
      for (const code of ["WO_EXPECTED_DURATION_UNREADABLE", "WO_REMAINING_TIME_INVALID"]) {
        const text = formatOrgIssueWithI18n({ code, message: "english", params: { value: "x" } });
        expect(text).toBe(String(i18n.t(`training:errors.${code}`, { value: "x" })));
        expect(text).not.toBe("english");
      }
    },
  );

  it.each(["fr", "en"])("asks to re-plan an unreadable planned end in %s", async (language) => {
    await i18n.changeLanguage(language);
    await i18n.loadNamespaces("training");
    const text = formatOrgIssueWithI18n({
      code: "WO_PLANNED_END_UNREADABLE",
      message: "The work order's planned end (someday) ...",
      params: { value: "someday" },
    });
    expect(text).toContain("someday");
    expect(text).toBe(
      String(i18n.t("training:errors.WO_PLANNED_END_UNREADABLE", { value: "someday" })),
    );
    expect(text).not.toBe("The work order's planned end (someday) ...");
  });
});

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
});

import { describe, expect, it } from "vitest";

import { formatOrgValidationIssue } from "@/lib/format-org-validation-issue";

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

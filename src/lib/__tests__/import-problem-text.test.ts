/**
 * Import problems are said from their code and details; without a code this version can say,
 * the screen keeps the server's sentence.
 */
import i18next from "i18next";
import { describe, expect, it } from "vitest";

import enPersonnel from "@/i18n/locale-data/en/personnel.json";
import frReference from "@/i18n/locale-data/fr/reference.json";
import { importProblemText } from "@/lib/import-problem-text";

async function translator(lng: string, ns: string, catalog: object) {
  const i18n = i18next.createInstance();
  await i18n.init({
    lng,
    ns: [ns],
    defaultNS: ns,
    resources: { [lng]: { [ns]: catalog } },
    interpolation: { escapeValue: false },
  });
  return (key: string, opts?: Record<string, unknown>): string =>
    String(i18n.t(key as never, (opts ?? {}) as never));
}

describe("importProblemText", () => {
  it("fills the details into the user's language", async () => {
    const t = await translator("fr", "reference", frReference);
    expect(
      importProblemText(
        {
          code: "REF_IMPORT_DUPLICATE_IN_BATCH",
          params: { row: "4", code: "PUMP", firstRow: "2" },
          message: "Ligne 4: code 'PUMP' duplique dans le lot (premiere occurrence: ligne 2).",
        },
        t,
      ),
    ).toBe("Ligne 4 : le code « PUMP » figure déjà à la ligne 2 du fichier.");
    const en = await translator("en", "personnel", enPersonnel);
    expect(
      importProblemText(
        { code: "PERSONNEL_IMPORT_UNKNOWN_POSITION", params: { code: "NO-POS" }, message: "x" },
        en,
      ),
    ).toBe("Unknown position “NO-POS”.");
  });

  it("returns nothing for a problem without a code, or with a code this version does not know", async () => {
    const t = await translator("fr", "reference", frReference);
    expect(importProblemText({ code: "", params: {}, message: "ancien texte" }, t)).toBeNull();
    expect(
      importProblemText({ code: "REF_IMPORT_NEW_THING", params: {}, message: "x" }, t),
    ).toBeNull();
  });
});

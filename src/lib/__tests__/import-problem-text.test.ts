/**
 * Import problems are said from their code and details; without a code this version can say,
 * the screen keeps the server's sentence.
 */
import i18next from "i18next";
import { describe, expect, it } from "vitest";

import { i18n as appI18n, initI18n } from "@/i18n/config";
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

describe("importProblemText with a refused-unit detail", () => {
  it("translates the GATE code carried by the detail, never showing it", async () => {
    initI18n();
    await appI18n.changeLanguage("en");
    const t = (key: string, opts?: Record<string, unknown>): string =>
      key === "import.problem.REF_IMPORT_INVALID_UNIT"
        ? `Row ${String(opts?.["row"])}: unit refused. ${String(opts?.["detail"])}`
        : key;
    const text = importProblemText(
      {
        code: "REF_IMPORT_INVALID_UNIT",
        params: {
          row: "7",
          detail: "GATE_UNIT_IMPORT_REVIEW_FLAG: le statut « à revoir » ne s'importe pas.",
        },
        message: "Ligne 7",
      },
      t,
    );
    expect(text).toContain("Row 7: unit refused.");
    expect(text).not.toContain("GATE_");
    expect(text).toBe(
      `Row 7: unit refused. ${String(appI18n.t("errors:gateError.GATE_UNIT_IMPORT_REVIEW_FLAG"))}`,
    );
  });
});

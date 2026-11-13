import { describe, expect, it, vi } from "vitest";

import { buildLotoCardHtml, buildPermitLotoSheetHtml } from "../loto-card-print";

vi.mock("@/components/wo/WoPrintFiche", () => ({ printHtmlDocument: vi.fn() }));
vi.mock("@/export/templates/document-header", () => ({
  DOCUMENT_HEADER_PRINT_CSS: "",
  loadDocumentHeaderHtml: vi.fn().mockResolvedValue(""),
  wrapDocumentHeaderBlock: (html: string) =>
    html ? `<div class="doc-letterhead">${html}</div>` : "",
}));

const t = (key: string) => `[${key}]`;

describe("buildLotoCardHtml", () => {
  it("prints every isolation field with translated labels and escapes user text", () => {
    const html = buildLotoCardHtml(
      {
        permit_code: "PTW-2026-000041",
        equipment_label: "PMP-101 — Feed <pump>",
        energy_type: "electrical",
        isolation_id: 300,
        isolation_point: "MCC-1 breaker",
        lock_number: null,
        verifier_signature: "Sara Sup",
        zero_energy_note: "Essai de démarrage refusé, VAT 0 V",
        expires_at: null,
        forced_removal: false,
        forced_authorized_by: null,
        forced_removed_at: null,
        forced_reason: null,
      },
      t,
      "fr",
      "<p>Usine Nord</p>",
    );
    expect(html).toContain("[card.title]");
    expect(html).toContain("[card.doNotOperate]");
    expect(html).toContain("PTW-2026-000041");
    expect(html).toContain("PMP-101 — Feed &lt;pump&gt;");
    expect(html).not.toContain("<pump>");
    expect(html).toContain("MCC-1 breaker");
    expect(html).toContain("Sara Sup");
    expect(html).toContain("[card.zeroEnergy]");
    expect(html).toContain("Essai de démarrage refusé, VAT 0 V");
    expect(html).toContain('<div class="doc-letterhead"><p>Usine Nord</p></div>');
    // Missing lock number / expiry fall back to the translated "none" value.
    expect(html.match(/\[detail\.none\]/g)?.length).toBe(2);
    // Not a forced removal: no such line.
    expect(html).not.toContain("[card.forcedRemoval]");
  });

  it("prints a forced removal with who authorised it, when and why", () => {
    const html = buildLotoCardHtml(
      {
        permit_code: "PTW-2026-000041",
        equipment_label: "PMP-101",
        energy_type: "electrical",
        isolation_id: 300,
        isolation_point: "MCC-1 breaker",
        lock_number: "L-12",
        verifier_signature: "Sara Sup",
        // Verified before the zero-energy test was recorded.
        zero_energy_note: null,
        expires_at: "2099-01-01T00:00:00Z",
        forced_removal: true,
        forced_authorized_by: "Chef <Site>",
        forced_removed_at: "2026-10-10T17:00:00Z",
        forced_reason: "Fin de poste, travailleur parti",
      },
      t,
      "fr",
      "",
    );
    expect(html).toContain("[card.forcedRemoval]");
    expect(html).toContain("[isolation.zeroEnergyNotRecorded]");
    expect(html).toContain("[card.forcedRemovalYes]");
    expect(html).toContain("Chef &lt;Site&gt;");
    expect(html).toContain("Fin de poste, travailleur parti");
    expect(html).toContain("[card.forcedAt]");
  });
});

describe("buildPermitLotoSheetHtml (L4-17)", () => {
  const card = (id: number, point: string, verifier: string | null) => ({
    permit_code: "PTW-2026-000041",
    equipment_label: "PMP-101 — Feed pump",
    energy_type: "electrical",
    isolation_id: id,
    isolation_point: point,
    lock_number: `L-${id}`,
    verifier_signature: verifier,
    zero_energy_note: null,
    expires_at: null,
    forced_removal: false,
    forced_authorized_by: null,
    forced_removed_at: null,
    forced_reason: null,
  });

  it("puts every point of the permit on one sheet, with the verifier's name", () => {
    const html = buildPermitLotoSheetHtml(
      [card(1, "MCC-1 breaker", "Sara Belkacem"), card(2, "Valve <V-12>", null)],
      t,
      "fr",
      "",
    );
    expect(html.match(/PTW-2026-000041/g)?.length).toBe(2); // title and header row, once each
    expect(html).toContain("MCC-1 breaker");
    expect(html).toContain("Valve &lt;V-12&gt;");
    expect(html).toContain("Sara Belkacem");
    expect(html).toContain("L-1");
    expect(html).toContain("L-2");
    expect(html).toContain("[card.signature]");
  });
});

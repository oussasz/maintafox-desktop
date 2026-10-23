import { describe, expect, it } from "vitest";

import { outstandingPublishIssues } from "@/components/lookups/PublishReadinessPanel";
import {
  buildReferenceCsv,
  parseReferenceCsv,
  splitCsvRecords,
} from "@/components/lookups/reference-csv";
import {
  pendingCandidateSet,
  referenceDomainLabel,
  referenceSetStatusLabelKey,
} from "@/components/lookups/reference-set-ui";
import type { RefExportRow, ReferenceSet, ReferenceValue } from "@shared/ipc-types";

function value(overrides: Partial<ReferenceValue>): ReferenceValue {
  return {
    id: 1,
    set_id: 10,
    parent_id: null,
    code: "PUMP",
    label: "Pompe",
    description: null,
    sort_order: 1,
    color_hex: null,
    icon_name: null,
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: null,
    ...overrides,
  };
}

describe("reference CSV", () => {
  it("round-trips labels with commas, quotes, accents and line breaks", () => {
    const rows: RefExportRow[] = [
      { value: value({ id: 1, code: "PRESS", label: 'Presse "hydraulique", 200 t' }), aliases: [] },
      {
        value: value({
          id: 2,
          code: "PRESS_SMALL",
          label: "Presse légère",
          description: "Ligne 1\nLigne 2",
          parent_id: 1,
          sort_order: 2,
        }),
        aliases: [],
      },
    ];
    const csv = buildReferenceCsv(rows);
    expect(csv.startsWith("﻿code,label,")).toBe(true);

    const parsed = parseReferenceCsv(csv);
    expect(parsed).toHaveLength(2);
    expect(parsed[0]).toMatchObject({ code: "PRESS", label: 'Presse "hydraulique", 200 t' });
    expect(parsed[1]).toMatchObject({
      code: "PRESS_SMALL",
      label: "Presse légère",
      description: "Ligne 1\nLigne 2",
      parent_code: "PRESS",
      sort_order: 2,
    });
  });

  it("accepts columns in any order and skips blank lines", () => {
    const parsed = parseReferenceCsv("label,code\r\n\r\nChaudière,BOILER\r\n");
    expect(parsed).toEqual([{ label: "Chaudière", code: "BOILER" }]);
  });

  it("splits quoted fields", () => {
    expect(splitCsvRecords('a,"b,c",d\n')).toEqual([["a", "b,c", "d"]]);
  });
});

describe("reference set presentation", () => {
  const s = (id: number, version_no: number, status: string): ReferenceSet => ({
    id,
    domain_id: 1,
    version_no,
    status,
    effective_from: null,
    created_by_id: null,
    created_at: "2026-10-01T00:00:00Z",
    published_at: null,
  });

  it("finds the version in progress", () => {
    expect(pendingCandidateSet([s(1, 1, "published")])).toBeNull();
    expect(
      pendingCandidateSet([s(1, 1, "superseded"), s(2, 2, "published"), s(3, 3, "draft")])?.id,
    ).toBe(3);
    expect(pendingCandidateSet([s(4, 4, "validated")])?.id).toBe(4);
  });

  it("names statuses and synthetic domains through translations", () => {
    expect(referenceSetStatusLabelKey("published")).toBe("browser.status.published");
    expect(referenceSetStatusLabelKey("weird")).toBe("browser.status.unknown");
    const t = (key: string) => `t:${key}`;
    expect(referenceDomainLabel({ id: -104, code: "WORK.URGENCY_SCALE", name: "x" }, t)).toBe(
      "t:synthetic.domains.urgencyScale",
    );
    expect(referenceDomainLabel({ id: 5, code: "EQUIPMENT.CLASS", name: "Classes" }, t)).toBe(
      "Classes",
    );
    // A list Maintafox provides is named from the translations when one exists.
    const known = (key: string) =>
      key === "domainLabels.EQUIPMENT_CLASS" ? "Equipment classes" : key;
    expect(referenceDomainLabel({ id: 5, code: "EQUIPMENT.CLASS", name: "Classes" }, known)).toBe(
      "Equipment classes",
    );
    expect(referenceDomainLabel({ id: 9, code: "PLANT.OWN_LIST", name: "Ma liste" }, known)).toBe(
      "Ma liste",
    );
    // The app's i18n returns "[key]" for a missing key (parseMissingKeyHandler).
    const bracketed = (key: string) => `[${key}]`;
    expect(
      referenceDomainLabel({ id: 9, code: "PLANT.OWN_LIST", name: "Ma liste" }, bracketed),
    ).toBe("Ma liste");
  });

  it("hides the readiness checks the publish action takes care of", () => {
    const issues = [
      { check: "set_status", message: "", severity: "blocker" },
      { check: "validation_report_missing", message: "", severity: "blocker" },
      { check: "unresolved_migrations", message: "", severity: "blocker" },
      { check: "impact_preview_required", message: "", severity: "blocker" },
    ];
    expect(outstandingPublishIssues(issues, "draft").map((i) => i.check)).toEqual([
      "unresolved_migrations",
    ]);
    expect(outstandingPublishIssues(issues, "validated").map((i) => i.check)).toEqual([
      "set_status",
      "validation_report_missing",
      "unresolved_migrations",
    ]);
  });
});

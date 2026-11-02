import { describe, expect, it } from "vitest";

import {
  articleExportRows,
  assetExportRows,
  permitExportRows,
  requestExportRows,
  workOrderExportRows,
} from "@/export/list-exports";
import type { InventoryArticleRow } from "@/services/inventory-service";
import type { WorkPermitRecord } from "@/services/permit-service";

describe("workOrderExportRows", () => {
  it("exports labels, never ids or codes", () => {
    const rows = workOrderExportRows(
      [
        {
          code: "OT-0001",
          title: "Changer le roulement",
          asset_label: "P-101 — Pompe",
          type_code: "corrective",
          type_label: "Corrective",
          status_code: "in_progress",
          lifecycle_state: "executing",
          responsible_display_name: null,
          responsible_username: "jdupont",
          planned_end: "2026-11-05T10:00:00Z",
        },
        {
          code: "OT-0002",
          title: "Graissage",
          asset_label: null,
          type_code: "preventive",
          type_label: null,
          status_code: null,
          lifecycle_state: null,
          responsible_display_name: null,
          responsible_username: null,
          planned_end: null,
        },
      ] as never,
      {
        statusLabel: (c) => ({ in_progress: "En cours", draft: "Brouillon" })[c] ?? c,
        lifecycleLabel: (c) => (c === "executing" ? "Exécution" : null),
        typeLabel: (c, f) => f ?? (c === "preventive" ? "Préventive" : c),
        formatDate: () => "05/11/2026",
      },
    );
    expect(rows).toEqual([
      [
        "OT-0001",
        "Changer le roulement",
        "P-101 — Pompe",
        "Corrective",
        "En cours",
        "Exécution",
        "jdupont",
        "05/11/2026",
      ],
      ["OT-0002", "Graissage", "", "Préventive", "Brouillon", "", "", ""],
    ]);
  });
});

describe("requestExportRows", () => {
  it("uses the validated urgency, hides a completed SLA and shows the work order", () => {
    const base = {
      code: "DI-1",
      title: "Fuite",
      asset_label: null,
      asset_code: "P-1",
      status: "submitted",
      reported_urgency: "high",
      validated_urgency: null,
      sla_state: "completed",
      submitter_display_name: "Awa",
      submitted_at: "2026-11-01T08:00:00Z",
      converted_to_wo_code: "OT-9",
    };
    const rows = requestExportRows(
      [base, { ...base, code: "DI-2", validated_urgency: "low", sla_state: "at_risk" }] as never,
      {
        statusLabel: () => "Soumise",
        urgencyLabel: (c) => ({ high: "Haute", low: "Basse" })[c] ?? c,
        slaLabel: () => "À risque",
        personLabel: (n) => n ?? "",
        formatDate: () => "01/11/2026",
      },
    );
    expect(rows[0]).toEqual([
      "DI-1",
      "Fuite",
      "P-1",
      "Soumise",
      "Haute",
      "",
      "Awa",
      "01/11/2026",
      "OT-9",
    ]);
    expect(rows[1]?.[4]).toBe("Basse");
    expect(rows[1]?.[5]).toBe("À risque");
  });
});

describe("assetExportRows", () => {
  it("resolves status and criticality and formats the meter", () => {
    const rows = assetExportRows(
      [
        {
          asset_code: "P-101",
          asset_name: "Pompe",
          class_name: "Pompes",
          criticality_code: "A",
          status_code: "IN_SERVICE",
          org_path: "Usine > Atelier",
          org_node_name: "Atelier",
          parent_asset_code: null,
          primary_meter_name: "Heures",
          primary_meter_reading: 1250.5,
          primary_meter_unit: "h",
        },
        {
          asset_code: "P-102",
          asset_name: "Pompe 2",
          class_name: null,
          criticality_code: null,
          status_code: "IN_SERVICE",
          org_path: null,
          org_node_name: "Atelier",
          parent_asset_code: "P-101",
          primary_meter_name: null,
          primary_meter_reading: null,
          primary_meter_unit: null,
        },
      ],
      {
        statusLabel: () => "En service",
        criticalityLabel: (c) => `${c} — Critique`,
        formatReading: (v, u) => `${String(v).replace(".", ",")} ${u ?? ""}`.trim(),
      },
    );
    expect(rows[0]).toEqual([
      "P-101",
      "Pompe",
      "Pompes",
      "A — Critique",
      "En service",
      "Usine > Atelier",
      "",
      "1250,5 h",
    ]);
    expect(rows[1]).toEqual(["P-102", "Pompe 2", "", "", "En service", "Atelier", "P-101", ""]);
  });
});

describe("articleExportRows", () => {
  const labels = {
    formatQty: (v: number) => String(v).replace(".", ","),
    formatCurrency: (v: number) => `${String(v).replace(".", ",")} €`,
    belowReorderLabel: "À commander",
    activeLabel: "Actif",
    inactiveLabel: "Inactif",
    isBelowReorder: (a: InventoryArticleRow) => a.available_qty <= a.reorder_point,
  };
  const article = {
    article_code: "ROU-6204",
    article_name: "Roulement 6204",
    family_name: "Roulements",
    family_code: "ROU",
    unit_code: "pce",
    on_hand_qty: 4,
    reserved_qty: 1.5,
    available_qty: 2.5,
    avg_unit_cost: 12.5,
    stock_value: 50,
    reorder_point: 3,
    is_active: 1,
  } as unknown as InventoryArticleRow;

  it("shows quantities and costs as the screen does and flags the articles to order", () => {
    const [row] = articleExportRows([article], labels);
    expect(row).toEqual([
      "ROU-6204",
      "Roulement 6204",
      "Roulements",
      "4 pce",
      "1,5",
      "2,5",
      "12,5 €",
      "50 €",
      "3",
      "À commander",
    ]);
  });

  it("leaves unvalued cells empty and marks inactive articles", () => {
    const [row] = articleExportRows(
      [{ ...article, is_active: 0, avg_unit_cost: null, stock_value: null, reorder_point: 0 }],
      labels,
    );
    expect(row?.slice(6)).toEqual(["", "", "", "Inactif"]);
  });
});

describe("permitExportRows", () => {
  it("resolves status, type, equipment and expiry", () => {
    const permit = {
      code: "PTW-1",
      status: "issued",
      permit_type_code: "loto",
      permit_type_name: "LOTO",
      linked_work_order_code: "OT-4",
      expires_at: "2026-11-02T18:00:00Z",
    } as WorkPermitRecord;
    const labels = {
      statusLabel: () => "Délivré",
      typeLabel: (_c: string | null, n: string | null) => n ?? "",
      assetLabel: () => "P-101 — Pompe",
      formatDateTime: () => "02/11/2026 18:00",
    };
    const rows = permitExportRows(
      [
        permit,
        { ...permit, permit_type_name: null, linked_work_order_code: null, expires_at: null },
      ],
      labels,
    );
    expect(rows[0]).toEqual([
      "PTW-1",
      "Délivré",
      "LOTO",
      "P-101 — Pompe",
      "OT-4",
      "02/11/2026 18:00",
    ]);
    expect(rows[1]).toEqual(["PTW-1", "Délivré", "", "P-101 — Pompe", "", ""]);
  });
});

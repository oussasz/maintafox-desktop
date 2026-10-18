import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  addAssetSparePart,
  assessAssetCriticality,
  getAssetCriticalityMethod,
  getAssetMaintenanceProfile,
  listArticleAssetUsage,
  listAssetCriticalityAssessments,
  listAssetSpareParts,
  previewAssetCriticality,
  removeAssetSparePart,
  updateAssetCommercial,
  updateAssetSparePart,
} from "../asset-registry-service";

// ── Fixtures (shapes of the Rust DTOs) ────────────────────────────────────────

const asset = {
  id: 7,
  entity_sync_id: "eq-7",
  asset_code: "PMP-101",
  asset_name: "Pompe d'alimentation",
  class_id: 1,
  class_code: "PUMP",
  class_name: "Pompe",
  family_code: null,
  family_name: null,
  subfamily_code: null,
  subfamily_name: null,
  criticality_value_id: 3,
  criticality_code: "A",
  status_code: "ACTIVE_IN_SERVICE",
  manufacturer: "KSB",
  model: null,
  serial_number: null,
  maintainable_boundary: true,
  rams_schedule_reference_value_id: null,
  rams_schedule_reference_value_name: null,
  rams_utilization_factor: 1,
  org_node_id: 2,
  org_node_name: "Chaufferie",
  commissioned_at: null,
  decommissioned_at: null,
  purchase_date: "2024-01-10",
  warranty_expiry_date: "2026-01-10",
  replacement_value: 125000,
  cost_center_code: "CC-MNT",
  org_cost_center_code: "CC-PRD",
  notes: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
  deleted_at: null,
  row_version: 4,
};

const sparePart = {
  id: 11,
  asset_id: 7,
  article_id: 31,
  article_code: "BRG-6205",
  article_name: "Roulement 6205",
  unit_label: "Unité",
  article_is_active: true,
  quantity: 2,
  position_label: "Palier côté accouplement",
  is_critical_spare: true,
  notes: null,
  on_hand_qty: 1,
  available_qty: 1,
  min_stock: 2,
  row_version: 1,
};

const assessment = {
  id: 5,
  asset_id: 7,
  method_code: "RISK_5X5_V1",
  safety_score: 2,
  environment_score: 1,
  production_score: 5,
  quality_score: 2,
  cost_score: 3,
  likelihood_score: 4,
  risk_score: 20,
  resulting_class: "A",
  previous_class: "C",
  rationale: "Seule pompe d'alimentation",
  assessed_by_name: "Admin",
  assessed_at: "2026-10-14T08:00:00Z",
};

const scores = { safety: 2, environment: 1, production: 5, quality: 2, cost: 3, likelihood: 4 };

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("commercial data", () => {
  it("sends the full input with the expected row version and keeps the new fields", async () => {
    mockInvoke.mockResolvedValueOnce(asset);
    const input = {
      purchase_date: "2024-01-10",
      warranty_expiry_date: "2026-01-10",
      replacement_value: 125000,
      cost_center_code: "CC-MNT",
      notes: null,
    };
    const updated = await updateAssetCommercial(7, input, 3);
    expect(mockInvoke).toHaveBeenCalledWith("update_asset_commercial", {
      assetId: 7,
      input,
      expectedRowVersion: 3,
    });
    expect(updated.warranty_expiry_date).toBe("2026-01-10");
    expect(updated.org_cost_center_code).toBe("CC-PRD");
  });
});

describe("maintenance profile", () => {
  it("passes the roll-up flag and parses the cost breakdown", async () => {
    mockInvoke.mockResolvedValueOnce({
      asset_id: 7,
      include_descendants: true,
      asset_count: 3,
      work_order_count: 4,
      open_work_order_count: 1,
      open_work_order_rule: "not_yet_closed",
      failure_work_order_count: 2,
      planned_work_order_count: 2,
      downtime_hours: 6.5,
      last_failure_at: "2026-09-01T00:00:00Z",
      cost_lifetime: { labor: 900, parts: 600, services: 0, total: 1500 },
      cost_recent: { labor: 300, parts: 200, services: 0, total: 500 },
      recent_window_months: 12,
      cost_by_year: [{ year: 2026, total: 500, work_order_count: 2 }],
      replacement_value: 10000,
      cost_to_replacement_ratio: 0.15,
      pm_plans: [
        {
          id: 3,
          code: "PM-0003",
          title: "Graissage",
          lifecycle_status: "active",
          next_due_at: null,
        },
      ],
    });
    const profile = await getAssetMaintenanceProfile(7, true);
    expect(mockInvoke).toHaveBeenCalledWith("get_asset_maintenance_profile", {
      assetId: 7,
      includeDescendants: true,
    });
    expect(profile.cost_lifetime.total).toBe(1500);
    expect(profile.pm_plans[0]?.code).toBe("PM-0003");
  });

  it("rejects a payload that does not match the contract", async () => {
    mockInvoke.mockResolvedValueOnce({ asset_id: 7 });
    await expect(getAssetMaintenanceProfile(7, false)).rejects.toThrow();
  });
});

describe("spare parts", () => {
  it("lists, adds, updates and removes lines with the right commands", async () => {
    mockInvoke.mockResolvedValueOnce([sparePart]);
    expect(await listAssetSpareParts(7)).toHaveLength(1);
    expect(mockInvoke).toHaveBeenLastCalledWith("list_asset_spare_parts", { assetId: 7 });

    const line = {
      quantity: 2,
      position_label: "Palier côté accouplement",
      is_critical_spare: true,
      notes: null,
    };
    mockInvoke.mockResolvedValueOnce(sparePart);
    await addAssetSparePart(7, 31, line);
    expect(mockInvoke).toHaveBeenLastCalledWith("add_asset_spare_part", {
      payload: { asset_id: 7, article_id: 31, line },
    });

    mockInvoke.mockResolvedValueOnce({ ...sparePart, quantity: 4, row_version: 2 });
    const updated = await updateAssetSparePart(11, { ...line, quantity: 4 }, 1);
    expect(updated.quantity).toBe(4);
    expect(mockInvoke).toHaveBeenLastCalledWith("update_asset_spare_part", {
      id: 11,
      line: { ...line, quantity: 4 },
      expectedRowVersion: 1,
    });

    mockInvoke.mockResolvedValueOnce(undefined);
    await removeAssetSparePart(11, 2);
    expect(mockInvoke).toHaveBeenLastCalledWith("remove_asset_spare_part", {
      id: 11,
      expectedRowVersion: 2,
    });
  });

  it("lists the assets that use an article", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        asset_id: 7,
        asset_code: "PMP-101",
        asset_name: "Pompe",
        quantity: 2,
        is_critical_spare: true,
      },
    ]);
    const usage = await listArticleAssetUsage(31);
    expect(usage[0]?.asset_code).toBe("PMP-101");
    expect(mockInvoke).toHaveBeenCalledWith("list_article_asset_usage", { articleId: 31 });
  });
});

describe("criticality", () => {
  it("reads the method thresholds and previews a result", async () => {
    mockInvoke.mockResolvedValueOnce({
      code: "RISK_5X5_V1",
      score_min: 1,
      score_max: 5,
      class_a_min_risk: 15,
      class_b_min_risk: 8,
      class_c_min_risk: 4,
      hse_override_score: 5,
    });
    const method = await getAssetCriticalityMethod();
    expect(method.class_a_min_risk).toBe(15);

    mockInvoke.mockResolvedValueOnce({
      method_code: "RISK_5X5_V1",
      consequence: 5,
      risk_score: 20,
      resulting_class: "A",
      hse_override: false,
    });
    const preview = await previewAssetCriticality(scores);
    expect(preview.resulting_class).toBe("A");
    expect(mockInvoke).toHaveBeenLastCalledWith("preview_asset_criticality", { scores });
  });

  it("refuses a class outside A–D", async () => {
    mockInvoke.mockResolvedValueOnce({
      method_code: "RISK_5X5_V1",
      consequence: 5,
      risk_score: 20,
      resulting_class: "Z",
      hse_override: false,
    });
    await expect(previewAssetCriticality(scores)).rejects.toThrow();
  });

  it("saves an assessment and returns the updated asset", async () => {
    mockInvoke.mockResolvedValueOnce({ assessment, asset: { ...asset, row_version: 5 } });
    const result = await assessAssetCriticality(7, scores, "Seule pompe", 4);
    expect(mockInvoke).toHaveBeenCalledWith("assess_asset_criticality", {
      payload: { asset_id: 7, scores, rationale: "Seule pompe", expected_row_version: 4 },
    });
    expect(result.asset.row_version).toBe(5);
    expect(result.assessment.risk_score).toBe(20);

    mockInvoke.mockResolvedValueOnce([assessment]);
    expect(await listAssetCriticalityAssessments(7)).toHaveLength(1);
  });
});

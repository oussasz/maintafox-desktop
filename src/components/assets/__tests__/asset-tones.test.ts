import { describe, expect, it } from "vitest";

import {
  criticalityLabelKey,
  criticalityRank,
  criticalityTone,
  isRetiredStatus,
  statusLabelKey,
  statusTone,
} from "@/components/assets/asset-tones";
import type { EquipmentTaxonomyCatalog, EquipmentTaxonomyOption } from "@shared/ipc-types";

const opt = (id: number, code: string, tag: string | null = null): EquipmentTaxonomyOption => ({
  id,
  code,
  label: `label ${code}`,
  parent_id: null,
  color_hex: null,
  semantic_tag: tag,
  is_system: true,
});

const CATALOG: EquipmentTaxonomyCatalog = {
  statuses: [
    opt(1, "ACTIVE_IN_SERVICE", "up"),
    opt(2, "UNDER_MAINTENANCE", "down"),
    opt(3, "IN_STOCK", "not_installed"),
    opt(4, "SCRAPPED", "retired"),
    opt(5, "SITE_STATUS", null),
  ],
  // List order = rank (the backend sorts by sort_order): X is the most critical here.
  criticalities: [opt(10, "X"), opt(11, "A"), opt(12, "B"), opt(13, "C")],
  classes: [],
  families: [],
  subfamilies: [],
};

describe("equipment status from the reference list", () => {
  it("takes the tone from the availability tag, never the code", () => {
    expect(statusTone(CATALOG, "active_in_service")).toBe("success");
    expect(statusTone(CATALOG, "UNDER_MAINTENANCE")).toBe("danger");
    expect(statusTone(CATALOG, "IN_STOCK")).toBe("info");
    expect(statusTone(CATALOG, "SCRAPPED")).toBe("neutral");
    expect(statusTone(CATALOG, "SITE_STATUS")).toBe("neutral");
    expect(statusTone(null, "ACTIVE_IN_SERVICE")).toBe("neutral");
  });

  it("calls retired what carries the retired tag", () => {
    expect(isRetiredStatus(CATALOG, "SCRAPPED")).toBe(true);
    expect(isRetiredStatus(CATALOG, "OUT_OF_SERVICE")).toBe(false);
    expect(isRetiredStatus(CATALOG, null)).toBe(false);
  });

  it("translates Maintafox values only", () => {
    expect(statusLabelKey(" active_in_service ")).toBe("status.operational");
    expect(statusLabelKey("SITE_STATUS")).toBeNull();
  });
});

describe("criticality from the reference list", () => {
  it("ranks by the list order", () => {
    expect(criticalityRank(CATALOG, "x")).toBe(0);
    expect(criticalityTone(CATALOG, "X")).toBe("danger");
    expect(criticalityTone(CATALOG, "A")).toBe("warning");
    expect(criticalityTone(CATALOG, "B")).toBe("info");
    expect(criticalityTone(CATALOG, "C")).toBe("neutral");
    expect(criticalityTone(CATALOG, "Z")).toBe("neutral");
    expect(criticalityRank(null, "A")).toBeNull();
  });

  it("translates the Maintafox levels only", () => {
    expect(criticalityLabelKey("a")).toBe("criticality.critical");
    expect(criticalityLabelKey("X")).toBeNull();
  });
});

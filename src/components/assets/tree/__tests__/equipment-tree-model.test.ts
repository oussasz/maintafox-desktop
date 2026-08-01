import { describe, expect, it } from "vitest";

import {
  buildEquipmentForest,
  collectAncestorIdsToReveal,
  collectExpandableIds,
  flattenVisibleRows,
  pruneExpandedIds,
} from "@/components/assets/tree/equipment-tree-model";
import type { AssetSearchResult } from "@shared/ipc-types";

function asset(
  id: number,
  code: string,
  parent_asset_id: number | null = null,
): AssetSearchResult {
  return {
    id,
    sync_id: `sync-${id}`,
    asset_code: code,
    asset_name: code,
    class_code: null,
    class_name: null,
    family_code: null,
    family_name: null,
    subfamily_code: null,
    subfamily_name: null,
    criticality_code: null,
    status_code: "ACTIVE",
    org_node_id: null,
    org_node_name: null,
    parent_asset_id,
    parent_asset_code: null,
    parent_asset_name: null,
    primary_meter_name: null,
    primary_meter_reading: null,
    primary_meter_unit: null,
    primary_meter_last_read_at: null,
    external_id_count: 0,
    row_version: 1,
  };
}

describe("buildEquipmentForest", () => {
  it("nests children under parents present in results", () => {
    const forest = buildEquipmentForest([
      asset(1, "COMP"),
      asset(2, "REG", 1),
      asset(3, "VALVE", 1),
    ]);
    expect(forest).toHaveLength(1);
    expect(forest[0]?.id).toBe(1);
    expect(forest[0]?.children.map((c) => c.id)).toEqual([2, 3]);
  });

  it("appears each asset only once", () => {
    const forest = buildEquipmentForest([
      asset(1, "A"),
      asset(2, "B", 1),
      asset(3, "C", 2),
    ]);
    const ids: number[] = [];
    const walk = (nodes: typeof forest) => {
      for (const n of nodes) {
        ids.push(n.id);
        walk(n.children);
      }
    };
    walk(forest);
    expect(ids.sort()).toEqual([1, 2, 3]);
  });

  it("promotes to root when parent is missing from results", () => {
    const forest = buildEquipmentForest([asset(2, "CHILD", 99)]);
    expect(forest).toHaveLength(1);
    expect(forest[0]?.id).toBe(2);
    expect(forest[0]?.children).toHaveLength(0);
  });

  it("preserves sibling order from results", () => {
    const forest = buildEquipmentForest([
      asset(1, "P"),
      asset(10, "Z", 1),
      asset(11, "A", 1),
    ]);
    expect(forest[0]?.children.map((c) => c.id)).toEqual([10, 11]);
  });

  it("breaks cycles by treating the cyclic edge as a root", () => {
    const forest = buildEquipmentForest([
      asset(1, "A", 2),
      asset(2, "B", 1),
    ]);
    const rootIds = forest.map((n) => n.id).sort();
    expect(rootIds).toEqual([1, 2]);
    expect(forest.every((n) => n.children.length === 0)).toBe(true);
  });

  it("supports unlimited nesting depth", () => {
    const rows = [
      asset(1, "L0"),
      asset(2, "L1", 1),
      asset(3, "L2", 2),
      asset(4, "L3", 3),
    ];
    const forest = buildEquipmentForest(rows);
    expect(forest[0]?.children[0]?.children[0]?.children[0]?.id).toBe(4);
  });
});

describe("flattenVisibleRows", () => {
  const forest = buildEquipmentForest([
    asset(1, "COMP"),
    asset(2, "REG", 1),
    asset(3, "SONDE", 2),
    asset(4, "MOTOR", 1),
  ]);

  it("hides children when parent is collapsed", () => {
    const rows = flattenVisibleRows(forest, new Set());
    expect(rows.map((r) => r.id)).toEqual([1]);
    expect(rows[0]?.childCount).toBe(2);
    expect(rows[0]?.isLeaf).toBe(false);
  });

  it("shows expanded branch with depth and guide flags", () => {
    const rows = flattenVisibleRows(forest, new Set([1, 2]));
    expect(rows.map((r) => r.id)).toEqual([1, 2, 3, 4]);
    expect(rows.map((r) => r.depth)).toEqual([0, 1, 2, 1]);
    const sonde = rows.find((r) => r.id === 3);
    expect(sonde?.ancestorIsLast).toEqual([true, false]);
    expect(sonde?.isLastSibling).toBe(true);
    const motor = rows.find((r) => r.id === 4);
    expect(motor?.isLastSibling).toBe(true);
    expect(motor?.ancestorIsLast).toEqual([true]);
  });
});

describe("expand helpers", () => {
  const forest = buildEquipmentForest([
    asset(1, "COMP"),
    asset(2, "REG", 1),
    asset(3, "SONDE", 2),
  ]);

  it("collectExpandableIds returns all parents", () => {
    expect(collectExpandableIds(forest).sort()).toEqual([1, 2]);
  });

  it("collectAncestorIdsToReveal matches expandable set for full reveal", () => {
    expect(collectAncestorIdsToReveal(forest).sort()).toEqual([1, 2]);
  });

  it("pruneExpandedIds drops stale ids", () => {
    const pruned = pruneExpandedIds(new Set([1, 2, 99]), forest);
    expect([...pruned].sort()).toEqual([1, 2]);
  });
});

/**
 * Client-side equipment forest from flat search results.
 *
 * Nest by parent_asset_id among the current result page only.
 * Missing parents become roots. Designed for flatten + memoization
 * (future: virtualization, lazy children, DnD re-parenting).
 */

import type { AssetSearchResult } from "@shared/ipc-types";

export type EquipmentTreeNode = {
  id: number;
  parentId: number | null;
  data: AssetSearchResult;
  children: EquipmentTreeNode[];
};

export type VisibleTreeRow = {
  id: number;
  depth: number;
  isLeaf: boolean;
  childCount: number;
  /** For each ancestor depth: whether that ancestor was last among siblings. */
  ancestorIsLast: boolean[];
  isLastSibling: boolean;
  data: AssetSearchResult;
};

/**
 * Build a forest from flat search rows. Each asset appears once.
 * Cycles and parents outside the result set are treated as roots.
 */
export function buildEquipmentForest(results: AssetSearchResult[]): EquipmentTreeNode[] {
  const byId = new Map<number, EquipmentTreeNode>();
  for (const data of results) {
    byId.set(data.id, {
      id: data.id,
      parentId: data.parent_asset_id ?? null,
      data,
      children: [],
    });
  }

  const roots: EquipmentTreeNode[] = [];
  const attached = new Set<number>();

  for (const data of results) {
    const node = byId.get(data.id);
    if (!node) continue;

    const parentId = data.parent_asset_id;
    if (parentId == null || !byId.has(parentId)) {
      roots.push(node);
      attached.add(node.id);
      continue;
    }

    // Cycle detection: walk ancestors in the result set; if we reach self, root.
    if (wouldCreateCycle(byId, node.id, parentId)) {
      roots.push(node);
      attached.add(node.id);
      continue;
    }

    const parent = byId.get(parentId);
    if (!parent) {
      roots.push(node);
      attached.add(node.id);
      continue;
    }

    parent.children.push(node);
    node.parentId = parentId;
    attached.add(node.id);
  }

  // Safety: any node never attached becomes a root (should not happen).
  for (const data of results) {
    if (!attached.has(data.id)) {
      const node = byId.get(data.id);
      if (node) roots.push(node);
    }
  }

  return roots;
}

function wouldCreateCycle(
  byId: Map<number, EquipmentTreeNode>,
  childId: number,
  parentId: number,
): boolean {
  let current: number | null = parentId;
  const seen = new Set<number>();
  while (current != null) {
    if (current === childId) return true;
    if (seen.has(current)) return true;
    seen.add(current);
    const node = byId.get(current);
    current = node?.parentId ?? null;
    // parentId on nodes may still point outside; stop if missing
    if (node && node.parentId != null && !byId.has(node.parentId)) {
      break;
    }
  }
  return false;
}

/** Depth-first flatten of expanded branches only. */
export function flattenVisibleRows(
  roots: EquipmentTreeNode[],
  expandedIds: ReadonlySet<number>,
): VisibleTreeRow[] {
  const rows: VisibleTreeRow[] = [];

  const visit = (
    nodes: EquipmentTreeNode[],
    depth: number,
    ancestorIsLast: boolean[],
  ) => {
    nodes.forEach((node, index) => {
      const isLastSibling = index === nodes.length - 1;
      const childCount = node.children.length;
      const isLeaf = childCount === 0;
      rows.push({
        id: node.id,
        depth,
        isLeaf,
        childCount,
        ancestorIsLast,
        isLastSibling,
        data: node.data,
      });

      if (!isLeaf && expandedIds.has(node.id)) {
        visit(node.children, depth + 1, [...ancestorIsLast, isLastSibling]);
      }
    });
  };

  visit(roots, 0, []);
  return rows;
}

/** All node ids that have at least one child (for Expand All). */
export function collectExpandableIds(roots: EquipmentTreeNode[]): number[] {
  const ids: number[] = [];
  const walk = (nodes: EquipmentTreeNode[]) => {
    for (const node of nodes) {
      if (node.children.length > 0) {
        ids.push(node.id);
        walk(node.children);
      }
    }
  };
  walk(roots);
  return ids;
}

/**
 * Ancestor ids that must be expanded so every node in the forest is reachable
 * when walking from roots (used when a search query is active).
 */
export function collectAncestorIdsToReveal(roots: EquipmentTreeNode[]): number[] {
  const ids: number[] = [];
  const walk = (nodes: EquipmentTreeNode[]) => {
    for (const node of nodes) {
      if (node.children.length > 0) {
        ids.push(node.id);
        walk(node.children);
      }
    }
  };
  walk(roots);
  return ids;
}

/** Keep only expanded ids that still exist in the forest and are expandable. */
export function pruneExpandedIds(
  expandedIds: ReadonlySet<number>,
  roots: EquipmentTreeNode[],
): Set<number> {
  const expandable = new Set(collectExpandableIds(roots));
  const next = new Set<number>();
  for (const id of expandedIds) {
    if (expandable.has(id)) next.add(id);
  }
  return next;
}

/** All asset ids present in the forest. */
export function collectAllIds(roots: EquipmentTreeNode[]): Set<number> {
  const ids = new Set<number>();
  const walk = (nodes: EquipmentTreeNode[]) => {
    for (const node of nodes) {
      ids.add(node.id);
      walk(node.children);
    }
  };
  walk(roots);
  return ids;
}

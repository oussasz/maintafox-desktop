/**
 * Equipment tree read from the database, level by level (`list_asset_tree_level`).
 *
 * The roots load with the registry filters; a branch loads its children the first time it is
 * expanded. Every level is paged (a "show more" row when `items < total`) and keeps its own
 * loading / error state, so a failed branch says so instead of looking empty. A change of
 * filters starts again from the roots.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { type AssetTreeRow, listAssetTreeLevel } from "@/services/asset-search-service";
import type { AssetSearchFilters } from "@shared/ipc-types";

/** Rows read per level and per "show more". */
export const TREE_PAGE_SIZE = 100;

export interface TreeLevelState {
  items: AssetTreeRow[];
  total: number;
  loading: boolean;
  error: unknown;
}

/** Key of a level: the parent id, or "root". */
export type LevelKey = number | "root";

export type FlatTreeRow =
  | {
      kind: "asset";
      row: AssetTreeRow;
      depth: number;
      isLastSibling: boolean;
      ancestorIsLast: boolean[];
    }
  | {
      kind: "status";
      parent: LevelKey;
      depth: number;
      state: "loading" | "error" | "more";
      error?: unknown;
      remaining?: number;
    };

export function useEquipmentTree(filters: AssetSearchFilters) {
  const [levels, setLevels] = useState<Map<LevelKey, TreeLevelState>>(new Map());
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const levelsRef = useRef(levels);
  levelsRef.current = levels;
  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;
  /** Bumps on every filter change: answers for older filters are ignored. */
  const generation = useRef(0);
  const filtersKey = JSON.stringify({ ...filters, limit: undefined, offset: undefined });

  const loadLevel = useCallback(
    async (key: LevelKey, append: boolean) => {
      const gen = generation.current;
      const offset = append ? (levelsRef.current.get(key)?.items.length ?? 0) : 0;
      setLevels((prev) => {
        const next = new Map(prev);
        const current = prev.get(key);
        next.set(key, {
          items: append && current ? current.items : [],
          total: current?.total ?? 0,
          loading: true,
          error: null,
        });
        return next;
      });
      try {
        const level = await listAssetTreeLevel(
          { ...filters, limit: TREE_PAGE_SIZE, offset },
          key === "root" ? null : key,
        );
        if (gen !== generation.current) return;
        setLevels((prev) => {
          const next = new Map(prev);
          const before = append ? (prev.get(key)?.items ?? []) : [];
          next.set(key, {
            items: [...before, ...level.items],
            total: level.total,
            loading: false,
            error: null,
          });
          return next;
        });
      } catch (err) {
        if (gen !== generation.current) return;
        setLevels((prev) => {
          const next = new Map(prev);
          const current = prev.get(key);
          next.set(key, {
            items: current?.items ?? [],
            total: current?.total ?? 0,
            loading: false,
            error: err,
          });
          return next;
        });
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps -- filtersKey stands for `filters`
    [filtersKey],
  );

  // New filters: start again from the roots.
  useEffect(() => {
    generation.current += 1;
    setLevels(new Map());
    setExpanded(new Set());
    void loadLevel("root", false);
  }, [loadLevel]);

  const toggle = useCallback(
    (id: number) => {
      const next = new Set(expandedRef.current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        if (!levelsRef.current.has(id)) void loadLevel(id, false);
      }
      setExpanded(next);
    },
    [loadLevel],
  );

  /** Expands every loaded branch that has children (each loads its own level). */
  const expandLoaded = useCallback(() => {
    const ids: number[] = [];
    for (const level of levels.values()) {
      for (const row of level.items) if (row.child_count > 0) ids.push(row.id);
    }
    setExpanded(new Set(ids));
    for (const id of ids) if (!levels.has(id)) void loadLevel(id, false);
  }, [levels, loadLevel]);

  const collapseAll = useCallback(() => setExpanded(new Set()), []);
  const retry = useCallback((key: LevelKey) => void loadLevel(key, false), [loadLevel]);
  const loadMore = useCallback((key: LevelKey) => void loadLevel(key, true), [loadLevel]);

  const rows = useMemo(() => {
    const out: FlatTreeRow[] = [];
    const visit = (key: LevelKey, depth: number, ancestorIsLast: boolean[]) => {
      const level = levels.get(key);
      if (!level) return;
      level.items.forEach((row, index) => {
        const isLastSibling = index === level.items.length - 1 && level.items.length >= level.total;
        out.push({ kind: "asset", row, depth, isLastSibling, ancestorIsLast });
        if (row.child_count > 0 && expanded.has(row.id)) {
          visit(row.id, depth + 1, [...ancestorIsLast, isLastSibling]);
        }
      });
      if (level.loading) out.push({ kind: "status", parent: key, depth, state: "loading" });
      else if (level.error != null)
        out.push({ kind: "status", parent: key, depth, state: "error", error: level.error });
      else if (level.items.length < level.total)
        out.push({
          kind: "status",
          parent: key,
          depth,
          state: "more",
          remaining: level.total - level.items.length,
        });
    };
    visit("root", 0, []);
    return out;
  }, [levels, expanded]);

  const root = levels.get("root");
  return {
    rows,
    expanded,
    toggle,
    expandLoaded,
    collapseAll,
    retry,
    loadMore,
    rootLoading: root == null || (root.loading && root.items.length === 0),
    rootError: root?.error ?? null,
    rootEmpty: root != null && !root.loading && root.error == null && root.items.length === 0,
  };
}

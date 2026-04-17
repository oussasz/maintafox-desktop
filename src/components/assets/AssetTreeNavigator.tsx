/**
 * AssetTreeNavigator.tsx
 *
 * Hierarchical tree view for the asset registry (left pane alternative to table).
 * Root-level assets load on mount; child expansion is lazy-loaded.
 * Keyboard navigation: Arrow keys, Enter to select, Right to expand, Left to collapse.
 */

import { ChevronDown, ChevronRight, Loader2, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset } from "@shared/ipc-types";

interface TreeNodeProps {
  asset: Asset;
  depth: number;
  onContextMenu?: ((e: React.MouseEvent, asset: Asset) => void) | undefined;
}

function TreeNode({ asset, depth, onContextMenu }: TreeNodeProps) {
  const expandedIds = useAssetStore((s) => s.treeExpandedIds);
  const selectedId = useAssetStore((s) => s.treeSelectedId);
  const children = useAssetStore((s) => s.treeChildren);
  const toggle = useAssetStore((s) => s.toggleTreeExpand);
  const selectNode = useAssetStore((s) => s.selectTreeNode);

  const isExpanded = expandedIds.has(asset.id);
  const isSelected = selectedId === asset.id;
  const childAssets = children.get(asset.id);

  const handleClick = () => {
    selectNode(asset.id);
  };

  const handleToggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    toggle(asset.id);
  };

  const handleContextMenuEvent = (e: React.MouseEvent) => {
    onContextMenu?.(e, asset);
  };

  return (
    <li role="treeitem" aria-expanded={isExpanded} aria-selected={isSelected}>
      <div
        className={cn(
          "flex items-center gap-1 px-2 py-1 text-xs cursor-pointer rounded-sm hover:bg-muted",
          isSelected && "bg-muted font-medium",
        )}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={handleClick}
        onContextMenu={handleContextMenuEvent}
        onKeyDown={(e) => {
          if (e.key === "Enter") handleClick();
          if (e.key === "ArrowRight" && !isExpanded) toggle(asset.id);
          if (e.key === "ArrowLeft" && isExpanded) toggle(asset.id);
        }}
        tabIndex={0}
        role="presentation"
      >
        <button
          type="button"
          className="shrink-0 p-0.5 rounded hover:bg-accent"
          onClick={handleToggle}
          tabIndex={-1}
          aria-label={isExpanded ? "Collapse" : "Expand"}
        >
          {isExpanded ? (
            <ChevronDown className="h-3.5 w-3.5 text-text-muted" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 text-text-muted" />
          )}
        </button>

        <span className="font-mono text-[10px] text-text-muted shrink-0">{asset.asset_code}</span>
        <span className="truncate">{asset.asset_name}</span>

        <div className="ml-auto flex items-center gap-1 shrink-0">
          <CriticalityBadge criticality={asset.criticality_code} compact />
          <Badge variant="outline" className="text-[9px] px-1">
            {asset.status_code}
          </Badge>
        </div>
      </div>

      {/* Children */}
      {isExpanded && childAssets && (
        <ul role="group">
          {childAssets.map((child) => (
            <TreeNode
              key={child.id}
              asset={child}
              depth={depth + 1}
              onContextMenu={onContextMenu}
            />
          ))}
        </ul>
      )}

      {/* Loading indicator for children */}
      {isExpanded && !childAssets && (
        <div style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }} className="py-1">
          <Loader2 className="h-3 w-3 animate-spin text-text-muted" />
        </div>
      )}
    </li>
  );
}

interface AssetTreeNavigatorProps {
  onContextMenu?: ((e: React.MouseEvent, asset: Asset) => void) | undefined;
}

export function AssetTreeNavigator({ onContextMenu }: AssetTreeNavigatorProps) {
  const { t } = useTranslation("equipment");
  const roots = useAssetStore((s) => s.treeRoots);
  const treeLoading = useAssetStore((s) => s.treeLoading);
  const loadRoots = useAssetStore((s) => s.loadTreeRoots);
  const treeRef = useRef<HTMLUListElement>(null);

  const [searchQuery, setSearchQuery] = useState("");

  useEffect(() => {
    void loadRoots();
  }, [loadRoots]);

  // Client-side filtering by code/name
  const filteredRoots = useMemo(() => {
    if (!searchQuery.trim()) return roots;
    const q = searchQuery.toLowerCase();
    return roots.filter(
      (a) => a.asset_code.toLowerCase().includes(q) || a.asset_name.toLowerCase().includes(q),
    );
  }, [roots, searchQuery]);

  // Keyboard navigation (arrow up/down)
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const focusable = treeRef.current?.querySelectorAll<HTMLElement>(
      "[role='presentation'][tabindex='0']",
    );
    if (!focusable) return;
    const arr = Array.from(focusable);
    const idx = arr.findIndex((el) => el === document.activeElement);
    const next = e.key === "ArrowDown" ? idx + 1 : idx - 1;
    if (next >= 0 && next < arr.length) {
      arr[next]?.focus();
    }
  }, []);

  return (
    <div className="flex h-full flex-col">
      {/* Search */}
      <div className="p-2 border-b border-surface-border">
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-text-muted" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t("tree.search")}
            className="pl-7 h-8 text-xs"
          />
        </div>
      </div>

      {/* Tree */}
      <div className="flex-1 overflow-auto">
        {treeLoading ? (
          <div className="flex items-center justify-center p-6">
            <Loader2 className="h-4 w-4 animate-spin text-text-muted" />
          </div>
        ) : filteredRoots.length === 0 ? (
          <div className="flex items-center justify-center p-6">
            <p className="text-xs text-text-muted">{t("tree.empty")}</p>
          </div>
        ) : (
          <ul
            ref={treeRef}
            role="tree"
            aria-label={t("tree.ariaLabel")}
            onKeyDown={handleKeyDown}
            className="py-1"
          >
            {filteredRoots.map((asset) => (
              <TreeNode key={asset.id} asset={asset} depth={0} onContextMenu={onContextMenu} />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

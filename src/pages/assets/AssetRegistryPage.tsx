/**
 * AssetRegistryPage.tsx
 *
 * Two-pane asset registry workspace.
 * Left pane: filter bar + result table OR tree navigator.
 * Right pane: detail panel for the selected asset.
 */

import { Cog, List, Plus, RefreshCw, TreePine } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetCreateForm } from "@/components/assets/AssetCreateForm";
import { AssetDecommissionModal } from "@/components/assets/AssetDecommissionModal";
import { AssetDetailPanel } from "@/components/assets/AssetDetailPanel";
import { AssetEditForm } from "@/components/assets/AssetEditForm";
import { AssetFilterBar } from "@/components/assets/AssetFilterBar";
import { AssetResultTable } from "@/components/assets/AssetResultTable";
import { AssetTreeContextMenu } from "@/components/assets/AssetTreeContextMenu";
import { AssetTreeNavigator } from "@/components/assets/AssetTreeNavigator";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset } from "@shared/ipc-types";

export function AssetRegistryPage() {
  const { t } = useTranslation("equipment");
  const selectedId = useAssetSearchStore((s) => s.selectedResultId);
  const results = useAssetSearchStore((s) => s.results);
  const loading = useAssetSearchStore((s) => s.loading);
  const runSearch = useAssetSearchStore((s) => s.runSearch);

  const treeMode = useAssetStore((s) => s.treeMode);
  const setTreeMode = useAssetStore((s) => s.setTreeMode);
  const treeSelectedId = useAssetStore((s) => s.treeSelectedId);
  const openCreateForm = useAssetStore((s) => s.openCreateForm);
  const openEditForm = useAssetStore((s) => s.openEditForm);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{
    asset: Asset;
    position: { x: number; y: number };
  } | null>(null);

  // Decommission from context menu
  const [decommissionTarget, setDecommissionTarget] = useState<Asset | null>(null);

  // Initial search on mount
  useEffect(() => {
    void runSearch();
  }, [runSearch]);

  const activeId = treeMode ? treeSelectedId : selectedId;

  const handleTreeContextMenu = useCallback((e: React.MouseEvent, asset: Asset) => {
    e.preventDefault();
    setContextMenu({ asset, position: { x: e.clientX, y: e.clientY } });
  }, []);

  return (
    <div className={mfLayout.moduleRoot}>
      {/* Page header — DI/OT pattern */}
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Cog className={mfLayout.moduleHeaderIcon} />
          <h1 className={mfLayout.moduleTitle}>{t("registry.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {t("registry.resultCount", { count: results.length })}
          </Badge>
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          {/* Tree / Table toggle — same control as DI/OT view switcher */}
          <div className={mfLayout.viewToggleGroup}>
            <Button
              variant={!treeMode ? "default" : "ghost"}
              size="sm"
              className={cn(mfLayout.viewToggleButton, "gap-1")}
              onClick={() => setTreeMode(false)}
            >
              <List className="h-3.5 w-3.5" />
              {t("registry.viewTable")}
            </Button>
            <Button
              variant={treeMode ? "default" : "ghost"}
              size="sm"
              className={cn(mfLayout.viewToggleButton, "gap-1")}
              onClick={() => setTreeMode(true)}
            >
              <TreePine className="h-3.5 w-3.5" />
              {t("registry.viewTree")}
            </Button>
          </div>

          <PermissionGate permission="eq.manage">
            <Button size="sm" className="gap-1.5" onClick={() => openCreateForm()}>
              <Plus className="h-3.5 w-3.5" />
              {t("createForm.button")}
            </Button>
          </PermissionGate>

          <Button
            variant="outline"
            size="sm"
            onClick={() => void runSearch()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            {t("registry.refresh")}
          </Button>
        </div>
      </div>

      {/* Two-pane workspace */}
      <div className={mfLayout.moduleWorkspaceSplit}>
        {/* Left pane */}
        <div className="flex flex-col w-[55%] min-w-[400px] border-r border-surface-border">
          {treeMode ? (
            <AssetTreeNavigator onContextMenu={handleTreeContextMenu} />
          ) : (
            <>
              <AssetFilterBar />
              <AssetResultTable />
            </>
          )}
        </div>

        {/* Right pane: detail panel */}
        <div className="flex-1 min-w-[300px]">
          {activeId ? (
            <AssetDetailPanel assetId={activeId} />
          ) : (
            <div className="flex h-full items-center justify-center p-6">
              <p className="text-sm text-text-muted">{t("registry.detail.noSelection")}</p>
            </div>
          )}
        </div>
      </div>

      {/* ── Dialogs ──────────────────────────────────────────────── */}
      <AssetCreateForm />
      <AssetEditForm />

      {/* Context menu */}
      {contextMenu && (
        <AssetTreeContextMenu
          asset={contextMenu.asset}
          position={contextMenu.position}
          onClose={() => setContextMenu(null)}
          onAddChild={(parent) => openCreateForm(parent)}
          onEdit={(a) => openEditForm(a)}
          onMove={() => {
            /* TODO: move dialog */
          }}
          onDecommission={(a) => setDecommissionTarget(a)}
        />
      )}

      {/* Decommission modal from tree context */}
      {decommissionTarget && (
        <AssetDecommissionModal
          open={!!decommissionTarget}
          asset={decommissionTarget}
          onClose={() => setDecommissionTarget(null)}
          onDecommissioned={() => {
            setDecommissionTarget(null);
            void runSearch();
          }}
        />
      )}
    </div>
  );
}

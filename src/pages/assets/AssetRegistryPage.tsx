/**
 * AssetRegistryPage.tsx
 *
 * Two-pane asset registry workspace.
 * Left pane: filter bar + result table.
 * Right pane: detail panel for the selected asset.
 */

import { Cog, RefreshCw } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { AssetDetailPanel } from "@/components/assets/AssetDetailPanel";
import { AssetFilterBar } from "@/components/assets/AssetFilterBar";
import { AssetResultTable } from "@/components/assets/AssetResultTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useAssetSearchStore } from "@/stores/asset-search-store";

export function AssetRegistryPage() {
  const { t } = useTranslation("equipment");
  const selectedId = useAssetSearchStore((s) => s.selectedResultId);
  const results = useAssetSearchStore((s) => s.results);
  const loading = useAssetSearchStore((s) => s.loading);
  const runSearch = useAssetSearchStore((s) => s.runSearch);

  // Initial search on mount
  useEffect(() => {
    void runSearch();
  }, [runSearch]);

  return (
    <div className="flex h-full flex-col">
      {/* Page header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-surface-border">
        <div className="flex items-center gap-3">
          <Cog className="h-5 w-5 text-text-muted" />
          <h1 className="text-xl font-semibold text-text-primary">{t("registry.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {t("registry.resultCount", { count: results.length })}
          </Badge>
        </div>
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

      {/* Two-pane workspace */}
      <div className="flex flex-1 min-h-0">
        {/* Left pane: filters + results */}
        <div className="flex flex-col w-[55%] min-w-[400px] border-r border-surface-border">
          <AssetFilterBar />
          <AssetResultTable />
        </div>

        {/* Right pane: detail panel */}
        <div className="flex-1 min-w-[300px]">
          {selectedId ? (
            <AssetDetailPanel assetId={selectedId} />
          ) : (
            <div className="flex h-full items-center justify-center p-6">
              <p className="text-sm text-text-muted">{t("registry.detail.noSelection")}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

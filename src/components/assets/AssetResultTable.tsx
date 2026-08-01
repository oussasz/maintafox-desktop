/**
 * AssetResultTable.tsx
 *
 * Search result grid for the asset registry.
 * Columns: code (tree), name, class, family, status, org, criticality.
 * Hierarchy is a filtered forest of the current search page (first column only).
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { EquipmentTreeCell } from "@/components/assets/tree/EquipmentTreeCell";
import { EquipmentTreeToolbar } from "@/components/assets/tree/EquipmentTreeToolbar";
import {
  buildEquipmentForest,
  flattenVisibleRows,
} from "@/components/assets/tree/equipment-tree-model";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import type { AssetSearchResult } from "@shared/ipc-types";

export function AssetResultTable() {
  const { t } = useTranslation("equipment");
  const results = useAssetSearchStore((s) => s.results);
  const selectedId = useAssetSearchStore((s) => s.selectedResultId);
  const loading = useAssetSearchStore((s) => s.loading);
  const error = useAssetSearchStore((s) => s.error);
  const expandedIds = useAssetSearchStore((s) => s.expandedIds);
  const selectAsset = useAssetSearchStore((s) => s.selectAsset);
  const toggleExpanded = useAssetSearchStore((s) => s.toggleExpanded);
  const expandAll = useAssetSearchStore((s) => s.expandAll);
  const collapseAll = useAssetSearchStore((s) => s.collapseAll);

  const forest = useMemo(() => buildEquipmentForest(results), [results]);
  const visibleRows = useMemo(() => flattenVisibleRows(forest, expandedIds), [forest, expandedIds]);

  const handleRowClick = (asset: AssetSearchResult) => {
    selectAsset(asset.id === selectedId ? null : asset.id);
  };

  if (error) {
    return (
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="text-sm text-status-danger">{error}</p>
      </div>
    );
  }

  if (!loading && results.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-6 text-center">
        <p className="text-sm text-text-muted">{t("empty.list")}</p>
        <p className="text-xs text-text-muted mt-1">{t("empty.listHint")}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <EquipmentTreeToolbar
        onExpandAll={expandAll}
        onCollapseAll={collapseAll}
        disabled={loading || results.length === 0}
      />
      <div className="flex-1 overflow-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[180px]">{t("list.columns.code")}</TableHead>
              <TableHead>{t("list.columns.name")}</TableHead>
              <TableHead className="w-[100px]">{t("list.columns.class")}</TableHead>
              <TableHead className="w-[100px]">{t("list.columns.family")}</TableHead>
              <TableHead className="w-[110px]">{t("list.columns.status")}</TableHead>
              <TableHead className="w-[140px]">{t("list.columns.site")}</TableHead>
              <TableHead className="w-[90px]">{t("list.columns.criticality")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {visibleRows.map((row) => {
              const asset = row.data;
              return (
                <TableRow
                  key={asset.id}
                  onClick={() => handleRowClick(asset)}
                  data-state={asset.id === selectedId ? "selected" : undefined}
                  className={cn("cursor-pointer", asset.id === selectedId && "bg-muted")}
                >
                  <TableCell className="py-1.5">
                    <EquipmentTreeCell
                      row={row}
                      isExpanded={expandedIds.has(row.id)}
                      onToggle={toggleExpanded}
                    />
                  </TableCell>
                  <TableCell className="text-sm">{asset.asset_name}</TableCell>
                  <TableCell className="text-xs text-text-muted">
                    {asset.class_name ?? "—"}
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">
                    {asset.family_name ?? "—"}
                  </TableCell>
                  <TableCell>
                    <AssetStatusBadge code={asset.status_code} />
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">
                    {asset.org_node_name ?? "—"}
                  </TableCell>
                  <TableCell>
                    <CriticalityBadge criticality={asset.criticality_code} compact />
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}

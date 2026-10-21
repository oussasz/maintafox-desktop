/**
 * AssetResultTable.tsx
 *
 * Equipment grid of the asset registry.
 * Columns: code (tree), name, class, family, status, org, criticality.
 * The hierarchy is read from the database level by level within the registry filters
 * (`list_asset_tree_level`): matching assets and their ancestors (shown dimmed as context),
 * children loaded when a branch is expanded — every branch is reachable on a large plant.
 */

import { useTranslation } from "react-i18next";

import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { CriticalityReviewMark } from "@/components/assets/CriticalityReviewMark";
import { EquipmentTreeCell } from "@/components/assets/tree/EquipmentTreeCell";
import { EquipmentTreeToolbar } from "@/components/assets/tree/EquipmentTreeToolbar";
import { useEquipmentTree } from "@/components/assets/tree/use-equipment-tree";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { mfTable } from "@/design-system/tokens";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import type { AssetSearchResult } from "@shared/ipc-types";

/** Indent of one tree level in the status rows (matches the tree guides). */
const LEVEL_INDENT_PX = 16;

export function AssetResultTable() {
  const { t } = useTranslation("equipment");
  const filters = useAssetSearchStore((s) => s.filters);
  const selectedId = useAssetSearchStore((s) => s.selectedResultId);
  const selectAsset = useAssetSearchStore((s) => s.selectAsset);
  const tree = useEquipmentTree(filters);

  // Opens the asset record (AssetDetailDialog); load errors are shown by the page.
  const handleRowClick = (asset: AssetSearchResult) => {
    selectAsset(asset.id);
  };

  if (tree.rootError != null && !tree.rows.some((r) => r.kind === "asset")) {
    return (
      <ErrorState
        error={tree.rootError}
        title={t("tree.loadError")}
        onRetry={() => tree.retry("root")}
      />
    );
  }

  if (tree.rootLoading) {
    return <LoadingState label={t("tree.loading")} />;
  }

  if (tree.rootEmpty) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-6 text-center">
        <p className="text-sm text-text-muted">{t("empty.list")}</p>
        <p className="mt-1 text-xs text-text-muted">{t("empty.listHint")}</p>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <EquipmentTreeToolbar onExpandAll={tree.expandLoaded} onCollapseAll={tree.collapseAll} />
      <div className="flex-1 overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className={mfTable.header}>
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
            {tree.rows.map((item) => {
              if (item.kind === "status") {
                return (
                  <TableRow key={`status-${String(item.parent)}`}>
                    <TableCell colSpan={7} className="py-1.5">
                      <div
                        className="flex flex-wrap items-center gap-2 text-xs"
                        style={{ paddingLeft: (item.depth + 1) * LEVEL_INDENT_PX }}
                      >
                        {item.state === "loading" ? (
                          <span className="text-text-muted" aria-busy="true">
                            {t("tree.loadingChildren")}
                          </span>
                        ) : item.state === "error" ? (
                          <>
                            <span role="alert" className="text-tone-danger-fg">
                              {t("tree.childrenError")} {describeAppError(item.error).title}
                            </span>
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              className="h-8"
                              onClick={() => tree.retry(item.parent)}
                            >
                              {t("tree.retry")}
                            </Button>
                          </>
                        ) : (
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            className="h-8"
                            onClick={() => tree.loadMore(item.parent)}
                          >
                            {t("tree.showMore", { count: item.remaining ?? 0 })}
                          </Button>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              }
              const asset = item.row;
              const row = {
                id: asset.id,
                depth: item.depth,
                isLeaf: asset.child_count === 0,
                childCount: asset.child_count,
                ancestorIsLast: item.ancestorIsLast,
                isLastSibling: item.isLastSibling,
                data: asset,
              };
              return (
                <TableRow
                  key={asset.id}
                  onClick={() => handleRowClick(asset)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleRowClick(asset);
                  }}
                  tabIndex={0}
                  data-state={asset.id === selectedId ? "selected" : undefined}
                  className={cn(
                    "cursor-pointer",
                    asset.id === selectedId && "bg-muted",
                    !asset.is_match && "text-text-muted",
                  )}
                  title={asset.is_match ? undefined : t("tree.contextRow")}
                >
                  <TableCell className="py-1.5">
                    <EquipmentTreeCell
                      row={row}
                      isExpanded={tree.expanded.has(asset.id)}
                      onToggle={tree.toggle}
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
                    <span className="inline-flex items-center gap-1">
                      <CriticalityBadge criticality={asset.criticality_code} compact />
                      {asset.criticality_to_review ? <CriticalityReviewMark /> : null}
                    </span>
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

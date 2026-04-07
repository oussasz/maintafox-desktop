/**
 * AssetResultTable.tsx
 *
 * Search result grid for the asset registry.
 * Columns: code, name, class, family, status, org, criticality.
 * Row click selects asset; status is rendered as a badge.
 */

import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
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

// ── Status → badge variant mapping ──────────────────────────────────────────

type BadgeVariant = "default" | "secondary" | "destructive" | "outline";

const STATUS_VARIANT: Record<string, BadgeVariant> = {
  ACTIVE: "default",
  OPERATIONAL: "default",
  STANDBY: "secondary",
  MAINTENANCE: "outline",
  DECOMMISSIONED: "destructive",
  SCRAPPED: "destructive",
};

function statusVariant(code: string): BadgeVariant {
  return STATUS_VARIANT[code] ?? "outline";
}

export function AssetResultTable() {
  const { t } = useTranslation("equipment");
  const results = useAssetSearchStore((s) => s.results);
  const selectedId = useAssetSearchStore((s) => s.selectedResultId);
  const loading = useAssetSearchStore((s) => s.loading);
  const error = useAssetSearchStore((s) => s.error);
  const selectAsset = useAssetSearchStore((s) => s.selectAsset);

  const handleRowClick = (asset: AssetSearchResult) => {
    selectAsset(asset.id === selectedId ? null : asset.id);
  };

  // Error state
  if (error) {
    return (
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="text-sm text-status-danger">{error}</p>
      </div>
    );
  }

  // Empty state
  if (!loading && results.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-6 text-center">
        <p className="text-sm text-text-muted">{t("empty.list")}</p>
        <p className="text-xs text-text-muted mt-1">{t("empty.listHint")}</p>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-[120px]">{t("list.columns.code")}</TableHead>
            <TableHead>{t("list.columns.name")}</TableHead>
            <TableHead className="w-[100px]">{t("list.columns.class")}</TableHead>
            <TableHead className="w-[100px]">{t("list.columns.family")}</TableHead>
            <TableHead className="w-[110px]">{t("list.columns.status")}</TableHead>
            <TableHead className="w-[140px]">{t("list.columns.site")}</TableHead>
            <TableHead className="w-[90px]">{t("list.columns.criticality")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {results.map((asset) => (
            <TableRow
              key={asset.id}
              onClick={() => handleRowClick(asset)}
              data-state={asset.id === selectedId ? "selected" : undefined}
              className={cn("cursor-pointer", asset.id === selectedId && "bg-muted")}
            >
              <TableCell className="font-mono text-xs">{asset.asset_code}</TableCell>
              <TableCell className="text-sm">{asset.asset_name}</TableCell>
              <TableCell className="text-xs text-text-muted">{asset.class_name ?? "—"}</TableCell>
              <TableCell className="text-xs text-text-muted">{asset.family_name ?? "—"}</TableCell>
              <TableCell>
                <StatusBadge code={asset.status_code} />
              </TableCell>
              <TableCell className="text-xs text-text-muted">
                {asset.org_node_name ?? "—"}
              </TableCell>
              <TableCell className="text-xs text-text-muted">
                {asset.criticality_code ?? "—"}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

// ── Status badge sub-component ──────────────────────────────────────────────

function StatusBadge({ code }: { code: string }) {
  const { t } = useTranslation("equipment");
  const variant = statusVariant(code);

  switch (code) {
    case "ACTIVE":
    case "OPERATIONAL":
      return (
        <Badge variant={variant} className="text-xs">
          {t("status.operational")}
        </Badge>
      );
    case "STANDBY":
      return (
        <Badge variant={variant} className="text-xs">
          {t("status.standby")}
        </Badge>
      );
    case "MAINTENANCE":
      return (
        <Badge variant={variant} className="text-xs">
          {t("status.maintenance")}
        </Badge>
      );
    case "DECOMMISSIONED":
      return (
        <Badge variant={variant} className="text-xs">
          {t("status.decommissioned")}
        </Badge>
      );
    case "SCRAPPED":
      return (
        <Badge variant={variant} className="text-xs">
          {t("status.scrapped")}
        </Badge>
      );
    default:
      return (
        <Badge variant="outline" className="text-xs">
          {code}
        </Badge>
      );
  }
}

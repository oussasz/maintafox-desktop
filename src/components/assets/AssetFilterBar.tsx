/**
 * AssetFilterBar.tsx
 *
 * Multi-criteria filter bar for asset search.
 * Provides query input, class/family/status/org filters, and clear/apply buttons.
 */

import { Search, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import type { AssetSearchFilters } from "@shared/ipc-types";

const ALL = "__all__";

export function AssetFilterBar() {
  const { t } = useTranslation("equipment");
  const filters = useAssetSearchStore((s) => s.filters);
  const loading = useAssetSearchStore((s) => s.loading);
  const updateFilters = useAssetSearchStore((s) => s.updateFilters);
  const clearFilters = useAssetSearchStore((s) => s.clearFilters);

  const [localQuery, setLocalQuery] = useState(filters.query ?? "");

  const handleApply = useCallback(() => {
    const partial: Partial<AssetSearchFilters> = {};
    const trimmed = localQuery.trim();
    partial.query = trimmed.length > 0 ? trimmed : null;
    void updateFilters(partial);
  }, [localQuery, updateFilters]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") handleApply();
    },
    [handleApply],
  );

  const handleClear = useCallback(() => {
    setLocalQuery("");
    void clearFilters();
  }, [clearFilters]);

  const handleSelectChange = useCallback(
    (key: keyof AssetSearchFilters, value: string) => {
      if (key === "classCodes" || key === "familyCodes" || key === "statusCodes") {
        void updateFilters({
          [key]: value === ALL ? null : [value],
        });
      }
    },
    [updateFilters],
  );

  const hasActiveFilters =
    !!filters.query ||
    !!filters.classCodes?.length ||
    !!filters.familyCodes?.length ||
    !!filters.statusCodes?.length;

  return (
    <div className="space-y-3 p-4 border-b border-surface-border">
      {/* Query input */}
      <div className="relative">
        <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
        <Input
          value={localQuery}
          onChange={(e) => setLocalQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t("registry.search.placeholder")}
          className="pl-9 h-9 text-sm"
        />
      </div>

      {/* Filter selects */}
      <div className="grid grid-cols-3 gap-2">
        {/* Class filter */}
        <div className="space-y-1">
          <label className="text-xs text-text-muted">{t("list.filters.class")}</label>
          <Select
            value={filters.classCodes?.[0] ?? ALL}
            onValueChange={(v) => handleSelectChange("classCodes", v)}
          >
            <SelectTrigger className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t("registry.filters.all")}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {/* Family filter */}
        <div className="space-y-1">
          <label className="text-xs text-text-muted">{t("list.columns.family")}</label>
          <Select
            value={filters.familyCodes?.[0] ?? ALL}
            onValueChange={(v) => handleSelectChange("familyCodes", v)}
          >
            <SelectTrigger className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t("registry.filters.all")}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {/* Status filter */}
        <div className="space-y-1">
          <label className="text-xs text-text-muted">{t("list.filters.status")}</label>
          <Select
            value={filters.statusCodes?.[0] ?? ALL}
            onValueChange={(v) => handleSelectChange("statusCodes", v)}
          >
            <SelectTrigger className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t("registry.filters.all")}</SelectItem>
              <SelectItem value="ACTIVE">{t("status.operational")}</SelectItem>
              <SelectItem value="STANDBY">{t("status.standby")}</SelectItem>
              <SelectItem value="MAINTENANCE">{t("status.maintenance")}</SelectItem>
              <SelectItem value="DECOMMISSIONED">{t("status.decommissioned")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2">
        <Button size="sm" onClick={handleApply} disabled={loading} className="h-8 text-xs gap-1.5">
          <Search className="h-3.5 w-3.5" />
          {t("registry.search.apply")}
        </Button>
        {hasActiveFilters && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClear}
            disabled={loading}
            className="h-8 text-xs gap-1.5"
          >
            <X className="h-3.5 w-3.5" />
            {t("registry.search.clear")}
          </Button>
        )}
      </div>
    </div>
  );
}

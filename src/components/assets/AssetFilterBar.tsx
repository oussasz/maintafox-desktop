/**
 * AssetFilterBar — equipment registry filters via SmartFilterBar.
 * Loads class/family options from published reference domains.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef, SmartFilterOption } from "@/components/filters/smart-filter-types";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import { useAssetSearchStore } from "@/stores/asset-search-store";

export function AssetFilterBar() {
  const { t } = useTranslation("equipment");
  const filters = useAssetSearchStore((s) => s.filters);
  const results = useAssetSearchStore((s) => s.results);
  const updateFilters = useAssetSearchStore((s) => s.updateFilters);
  const clearFilters = useAssetSearchStore((s) => s.clearFilters);

  const [searchInput, setSearchInput] = useState(filters.query ?? "");
  const [classOptions, setClassOptions] = useState<SmartFilterOption[]>([]);
  const [familyOptions, setFamilyOptions] = useState<SmartFilterOption[]>([]);
  const [statusOptions, setStatusOptions] = useState<SmartFilterOption[]>([]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [classes, families, statuses] = await Promise.all([
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.CLASS"),
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.FAMILY"),
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.STATUS"),
        ]);
        if (cancelled) return;
        setClassOptions(classes.map((v) => ({ value: v.code, label: v.label })));
        setFamilyOptions(families.map((v) => ({ value: v.code, label: v.label })));
        setStatusOptions(statuses.map((v) => ({ value: v.code, label: v.label })));
      } catch {
        if (!cancelled) {
          setClassOptions([]);
          setFamilyOptions([]);
          setStatusOptions([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const onSearchChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      void updateFilters({ query: trimmed.length > 0 ? trimmed : null });
    },
    [updateFilters],
  );

  const onReset = useCallback(() => {
    setSearchInput("");
    void clearFilters();
  }, [clearFilters]);

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "class",
        kind: "select",
        label: t("list.filters.class"),
        options: classOptions,
        value: filters.class_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ class_codes: v ? [v] : null }),
        allLabel: t("registry.filters.all"),
      },
      {
        id: "family",
        kind: "select",
        label: t("list.columns.family"),
        options: familyOptions,
        value: filters.family_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ family_codes: v ? [v] : null }),
        allLabel: t("registry.filters.all"),
      },
      {
        id: "status",
        kind: "select",
        label: t("list.filters.status"),
        options: statusOptions,
        value: filters.status_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ status_codes: v ? [v] : null }),
        allLabel: t("registry.filters.all"),
      },
    ],
    [t, classOptions, familyOptions, statusOptions, filters, updateFilters],
  );

  return (
    <SmartFilterBar
      searchPlaceholder={t("registry.search.placeholder")}
      searchValue={searchInput}
      onSearchInputChange={setSearchInput}
      onSearchChange={onSearchChange}
      filters={filterDefs}
      resultCount={results.length}
      resultCountLabel={t("registry.resultCount", { count: results.length })}
      onReset={onReset}
    />
  );
}

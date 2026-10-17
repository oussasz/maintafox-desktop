/**
 * AssetFilterBar — equipment registry filters via SmartFilterBar.
 * Class / family / status / criticality options come from the published reference lists; when they
 * cannot be loaded the bar says so with a retry instead of offering empty lists.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { criticalityLabelKey } from "@/components/assets/asset-tones";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef, SmartFilterOption } from "@/components/filters/smart-filter-types";
import { ErrorState } from "@/components/ui/data-state";
import { listOrgTree } from "@/services/org-node-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";
import type { AssetSearchFilters } from "@shared/ipc-types";

export function AssetFilterBar() {
  const { t } = useTranslation("equipment");
  const filters = useAssetSearchStore((s) => s.filters);
  const total = useAssetSearchStore((s) => s.total);
  const updateFilters = useAssetSearchStore((s) => s.updateFilters);
  const clearFilters = useAssetSearchStore((s) => s.clearFilters);

  const [searchInput, setSearchInput] = useState(filters.query ?? "");
  const [classOptions, setClassOptions] = useState<SmartFilterOption[]>([]);
  const [familyOptions, setFamilyOptions] = useState<SmartFilterOption[]>([]);
  const [statusOptions, setStatusOptions] = useState<SmartFilterOption[]>([]);
  const [locationOptions, setLocationOptions] = useState<SmartFilterOption[]>([]);
  const [optionsError, setOptionsError] = useState<unknown>(null);
  const [optionsReload, setOptionsReload] = useState(0);
  const { catalog } = useEquipmentTaxonomy();
  const criticalityOptions = useMemo<SmartFilterOption[]>(
    () =>
      (catalog?.criticalities ?? []).map((c) => {
        const key = criticalityLabelKey(c.code);
        return { value: c.code, label: `${c.code} — ${key ? t(key as never) : c.label}` };
      }),
    [catalog, t],
  );

  useEffect(() => {
    let cancelled = false;
    setOptionsError(null);
    void (async () => {
      try {
        const [classes, families, statuses, orgRows] = await Promise.all([
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.CLASS"),
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.FAMILY"),
          listPublishedReferenceValuesByDomainCode("EQUIPMENT.STATUS"),
          listOrgTree(),
        ]);
        if (cancelled) return;
        // Tree order (ancestor path); the indentation shows the level. Includes the whole subtree.
        setLocationOptions(
          orgRows
            .filter((r) => r.node.status === "active")
            .map((r) => ({
              value: String(r.node.id),
              label: `${"  ".repeat(Math.max(0, r.node.depth))}${r.node.name}`,
            })),
        );
        setClassOptions(classes.map((v) => ({ value: v.code, label: v.label })));
        setFamilyOptions(families.map((v) => ({ value: v.code, label: v.label })));
        setStatusOptions(statuses.map((v) => ({ value: v.code, label: v.label })));
      } catch (err) {
        if (!cancelled) setOptionsError(err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [optionsReload]);

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

  const focusOptions = useMemo<SmartFilterOption[]>(
    () => [
      { value: "status_down", label: t("metrics.statusSummary") },
      { value: "critical_at_risk", label: t("metrics.criticalAtRisk") },
      { value: "open_work", label: t("metrics.openWork") },
      { value: "stale_meters", label: t("metrics.staleMeters") },
    ],
    [t],
  );

  const filterDefs = useMemo<SmartFilterDef[]>(() => {
    const defs: SmartFilterDef[] = [
      {
        id: "class",
        kind: "select",
        label: t("list.filters.class"),
        options: classOptions,
        value: filters.class_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ class_codes: v ? [v] : null }),
      },
      {
        id: "family",
        kind: "select",
        label: t("list.columns.family"),
        options: familyOptions,
        value: filters.family_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ family_codes: v ? [v] : null }),
      },
      {
        id: "criticality",
        kind: "select",
        label: t("list.filters.criticality"),
        options: criticalityOptions,
        value: filters.criticality_codes?.[0] ?? null,
        onChange: (v) => void updateFilters({ criticality_codes: v ? [v] : null }),
      },
      {
        id: "location",
        kind: "select",
        label: t("list.filters.site"),
        options: locationOptions,
        value: filters.org_node_ids?.[0] != null ? String(filters.org_node_ids[0]) : null,
        onChange: (v) =>
          void updateFilters({ org_node_ids: v ? [Number(v)] : null, org_subtree: true }),
      },
      {
        id: "status",
        kind: "select",
        label: t("list.filters.status"),
        options: statusOptions,
        value: filters.status_codes?.[0] ?? null,
        onChange: (v) =>
          void updateFilters({
            status_codes: v ? [v] : null,
            ...(v ? { registry_focus: null } : {}),
          }),
      },
    ];
    // Chip + clear when a KPI drill focus is active (driven by metric strip).
    if (filters.registry_focus) {
      defs.push({
        id: "registry_focus",
        kind: "select",
        label: t("metrics.focusFilter"),
        options: focusOptions,
        value: filters.registry_focus,
        onChange: (v) =>
          void updateFilters({
            registry_focus: (v as AssetSearchFilters["registry_focus"]) ?? null,
          }),
      });
    }
    return defs;
  }, [
    t,
    classOptions,
    familyOptions,
    statusOptions,
    criticalityOptions,
    locationOptions,
    focusOptions,
    filters,
    updateFilters,
  ]);

  return (
    <div className="space-y-2">
      {optionsError != null ? (
        <ErrorState
          error={optionsError}
          title={t("registry.filterOptionsError")}
          onRetry={() => setOptionsReload((n) => n + 1)}
          className="py-3"
        />
      ) : null}
      <SmartFilterBar
        searchPlaceholder={t("registry.search.placeholder")}
        searchValue={searchInput}
        onSearchInputChange={setSearchInput}
        onSearchChange={onSearchChange}
        filters={filterDefs}
        resultCount={total}
        resultCountLabel={t("registry.resultCount", { count: total })}
        onReset={onReset}
      />
    </div>
  );
}

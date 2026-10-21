/**
 * AssetRegistryPage.tsx
 *
 * Equipment registry (OT/DI module pattern): header with count and actions, filter bar with the
 * KPI strip, then the list (flat, paged) or the structure tree. Selecting an asset opens its
 * record (AssetDetailDialog); `?assetId=<id>` opens a record from another screen.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { Cog, GitFork, List, Plus, Upload } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetCreateForm } from "@/components/assets/AssetCreateForm";
import { AssetDetailDialog } from "@/components/assets/AssetDetailDialog";
import { AssetEditForm } from "@/components/assets/AssetEditForm";
import { AssetFilterBar } from "@/components/assets/AssetFilterBar";
import { AssetResultTable } from "@/components/assets/AssetResultTable";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { CriticalityReviewMark } from "@/components/assets/CriticalityReviewMark";
import { EquipmentRegistryMetricCards } from "@/components/assets/EquipmentRegistryMetricCards";
import { DataTable } from "@/components/data/DataTable";
import { DiFormDialog } from "@/components/di/DiFormDialog";
import {
  FilterToggleButton,
  RefreshButton,
  type ViewOption,
  ViewToggle,
} from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { WoFormDialog } from "@/components/wo/WoFormDialog";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { LIST_PARAM, textParam, withoutParams } from "@/lib/list-links";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import { useAssetStore } from "@/stores/asset-store";
import type { AssetSearchResult } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type RegistryView = "list" | "tree";
const VIEW_KEY = "equipment-view-mode";
const FILTERS_KEY = "equipment-show-filters";

function readView(): RegistryView {
  try {
    return localStorage.getItem(VIEW_KEY) === "tree" ? "tree" : "list";
  } catch {
    return "list";
  }
}

function readShowFilters(): boolean {
  try {
    return localStorage.getItem(FILTERS_KEY) !== "0";
  } catch {
    return true;
  }
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preference only; the page works without storage.
  }
}

export function AssetRegistryPage() {
  const { t } = useTranslation("equipment");
  const { formatDecimal } = useFormatters();
  const navigate = useNavigate();
  const results = useAssetSearchStore((s) => s.results);
  const total = useAssetSearchStore((s) => s.total);
  const loading = useAssetSearchStore((s) => s.loading);
  const loadingMore = useAssetSearchStore((s) => s.loadingMore);
  const error = useAssetSearchStore((s) => s.error);
  const filters = useAssetSearchStore((s) => s.filters);
  const runSearch = useAssetSearchStore((s) => s.runSearch);
  const loadMore = useAssetSearchStore((s) => s.loadMore);
  const selectAsset = useAssetSearchStore((s) => s.selectAsset);
  const openCreateForm = useAssetStore((s) => s.openCreateForm);
  const showCreateForm = useAssetStore((s) => s.showCreateForm);

  const [view, setView] = useState<RegistryView>(readView);
  const [showFilters, setShowFilters] = useState(readShowFilters);
  const [searchParams, setSearchParams] = useSearchParams();

  useEffect(() => {
    void runSearch();
  }, [runSearch]);

  // A created asset appears once the create form closes.
  const createWasOpen = useRef(false);
  useEffect(() => {
    if (createWasOpen.current && !showCreateForm) void runSearch();
    createWasOpen.current = showCreateForm;
  }, [showCreateForm, runSearch]);

  // Deep link from other modules (org inspector, history, notifications): /equipment?assetId=42
  useEffect(() => {
    const raw = searchParams.get("assetId");
    if (!raw) return;
    const id = Number(raw);
    if (Number.isInteger(id) && id > 0) selectAsset(id);
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("assetId");
        return next;
      },
      { replace: true },
    );
  }, [searchParams, selectAsset, setSearchParams]);

  // A link may carry a status filter (?status=<code>): it becomes the registry's status filter,
  // shown as a chip the user can clear, then leaves the address.
  const updateFilters = useAssetSearchStore((s) => s.updateFilters);
  useEffect(() => {
    const status = textParam(searchParams.get(LIST_PARAM.status));
    if (!status) return;
    void updateFilters({ status_codes: [status], registry_focus: null });
    setSearchParams(withoutParams(searchParams, LIST_PARAM.status), { replace: true });
  }, [searchParams, setSearchParams, updateFilters]);

  const switchView = useCallback((v: RegistryView) => {
    setView(v);
    remember(VIEW_KEY, v);
  }, []);

  const toggleFilters = useCallback(() => {
    setShowFilters((prev) => {
      remember(FILTERS_KEY, prev ? "0" : "1");
      return !prev;
    });
  }, []);

  const viewOptions = useMemo<ViewOption<RegistryView>[]>(
    () => [
      { value: "list", label: t("registry.viewTable"), icon: List },
      { value: "tree", label: t("registry.viewTree"), icon: GitFork },
    ],
    [t],
  );

  const activeFilterCount = [
    filters.query,
    filters.class_codes?.length,
    filters.family_codes?.length,
    filters.status_codes?.length,
    filters.criticality_codes?.length,
    filters.org_node_ids?.length,
    filters.registry_focus,
  ].filter(Boolean).length;

  const columns = useMemo<ColumnDef<AssetSearchResult>[]>(
    () => [
      {
        accessorKey: "asset_code",
        header: t("list.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.asset_code}</span>,
      },
      {
        accessorKey: "asset_name",
        header: t("list.columns.name"),
        cell: ({ row }) => (
          <span className="block max-w-[260px] truncate">{row.original.asset_name}</span>
        ),
      },
      {
        id: "class",
        accessorFn: (a) => a.class_name ?? "",
        header: t("list.columns.class"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.class_name ?? "—"}</span>
        ),
      },
      {
        id: "criticality",
        accessorFn: (a) => a.criticality_code ?? "",
        header: t("list.columns.criticality"),
        cell: ({ row }) => (
          <span className="inline-flex items-center gap-1">
            <CriticalityBadge criticality={row.original.criticality_code} compact />
            {row.original.criticality_to_review ? <CriticalityReviewMark /> : null}
          </span>
        ),
      },
      {
        id: "status",
        accessorFn: (a) => a.status_code,
        header: t("list.columns.status"),
        cell: ({ row }) => <AssetStatusBadge code={row.original.status_code} />,
      },
      {
        id: "location",
        accessorFn: (a) => a.org_path ?? a.org_node_name ?? "",
        header: t("list.columns.site"),
        cell: ({ row }) => (
          <span
            className="block max-w-[220px] truncate text-xs text-text-muted"
            title={row.original.org_path ?? undefined}
          >
            {row.original.org_path ?? row.original.org_node_name ?? "—"}
          </span>
        ),
      },
      {
        id: "parent",
        accessorFn: (a) => a.parent_asset_code ?? "",
        header: t("list.columns.parentName"),
        cell: ({ row }) => {
          const parentId = row.original.parent_asset_id;
          if (parentId == null) return <span className="text-xs text-text-muted">—</span>;
          return (
            <Button
              variant="link"
              className="h-auto min-h-[32px] p-0 font-mono text-xs"
              onClick={(e) => {
                e.stopPropagation();
                selectAsset(parentId);
              }}
              title={row.original.parent_asset_name ?? undefined}
            >
              {row.original.parent_asset_code}
            </Button>
          );
        },
      },
      {
        id: "meter",
        accessorFn: (a) => a.primary_meter_reading ?? -1,
        header: t("list.columns.primaryMeter"),
        cell: ({ row }) =>
          row.original.primary_meter_reading != null ? (
            <span
              className="text-xs tabular-nums"
              title={row.original.primary_meter_name ?? undefined}
            >
              {formatDecimal(row.original.primary_meter_reading, 1)}{" "}
              {row.original.primary_meter_unit ?? ""}
            </span>
          ) : (
            <span className="text-xs text-text-muted">—</span>
          ),
      },
    ],
    [t, formatDecimal, selectAsset],
  );

  const showError = error != null && results.length === 0;

  return (
    <ModulePageShell
      icon={Cog}
      title={t("registry.title")}
      count={total}
      bodyClassName={null}
      actions={
        <>
          <PermissionGate permission={P.EQ_MANAGE}>
            <Button className={mfLayout.primaryAction} onClick={() => openCreateForm()}>
              <Plus className="h-4 w-4" aria-hidden />
              {t("createForm.button")}
            </Button>
          </PermissionGate>
          <PermissionGate permission={P.EQ_IMPORT}>
            <Button
              variant="outline"
              className="h-11 gap-1.5"
              onClick={() => void navigate("/equipment/import")}
            >
              <Upload className="h-4 w-4" aria-hidden />
              {t("registry.import")}
            </Button>
          </PermissionGate>
          <ViewToggle value={view} options={viewOptions} onChange={switchView} />
          <FilterToggleButton
            open={showFilters}
            onToggle={toggleFilters}
            activeCount={activeFilterCount}
          />
          <RefreshButton onRefresh={() => void runSearch()} loading={loading} />
        </>
      }
      filterBar={
        showFilters ? (
          <>
            <AssetFilterBar />
            <div className="border-b border-surface-border px-6 py-3">
              <EquipmentRegistryMetricCards />
            </div>
          </>
        ) : null
      }
    >
      {showError ? (
        <div className="p-4">
          <ErrorState error={error} onRetry={() => void runSearch()} retrying={loading} />
        </div>
      ) : (
        <>
          {results.length < total ? (
            <div className="px-4 pt-3">
              <TruncationNotice
                shown={results.length}
                total={total}
                onLoadMore={() => void loadMore()}
                loadingMore={loadingMore}
              />
            </div>
          ) : null}
          {view === "list" ? (
            <div className="p-4">
              {!loading && results.length === 0 ? (
                <div className="py-12 text-center">
                  <p className="text-sm text-text-muted">{t("empty.list")}</p>
                  <p className="mt-1 text-xs text-text-muted">{t("empty.listHint")}</p>
                </div>
              ) : (
                <DataTable
                  columns={columns}
                  data={results}
                  pageSize={20}
                  isLoading={loading}
                  skeletonRows={8}
                  onRowClick={(row) => selectAsset(row.id)}
                />
              )}
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col p-4">
              <AssetResultTable />
            </div>
          )}
        </>
      )}

      <AssetDetailDialog />
      <AssetCreateForm />
      <AssetEditForm />
      <WoFormDialog />
      <DiFormDialog />
    </ModulePageShell>
  );
}

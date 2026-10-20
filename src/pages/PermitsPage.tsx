/**
 * PermitsPage.tsx
 *
 * Work permits / LOTO on the module page anatomy (docs/DESIGN_SYSTEM.md §3):
 * header (count, create, types, filters, refresh) · SmartFilterBar (search + status/type) ·
 * KPI tiles · ErrorState | DataTable · permit detail in a centred dialog.
 *
 * The search runs on the server over every permit; `?permit=<id>` opens that permit by id even
 * when it is not in the loaded page (the OT permit gate links here). The KPIs load on their own:
 * a KPI failure never hides the list, and the list failure never shows zeros.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { CalendarCheck, HardHat, Plus, Settings2, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { FilterToggleButton, RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { PermitTypesHost } from "@/components/lookups/adapters/PermitTypesHost";
import { KpiTile } from "@/components/metrics/KpiTile";
import { ForcedRemovalReviewList } from "@/components/permits/ForcedRemovalReviewList";
import {
  PermitCreateDialog,
  type PermitCreateWorkOrder,
} from "@/components/permits/PermitCreateDialog";
import { PermitDetailPanel } from "@/components/permits/PermitDetailPanel";
import {
  PermitHiddenNotice,
  PermitNoSiteBadge,
  usePermitPlantWideAccess,
} from "@/components/permits/PermitSiteScope";
import {
  PERMIT_QUERY_CREATE_FOR_WO,
  PERMIT_QUERY_SELECT,
  PERMIT_REFUSAL_NAMESPACES,
  formatAssetLabel,
} from "@/components/permits/permit-rules";
import { usePermitTypeName } from "@/components/permits/permit-type-name";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { PERMIT_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { i18n } from "@/i18n";
import { listLinks } from "@/lib/list-links";
import {
  type PermitComplianceKpi30dRecord,
  type PermitTypeRecord,
  type WorkPermitRecord,
  getWorkPermit,
  listOpenPermitsReport,
  listPermitTypes,
  listWorkPermits,
  permitComplianceKpi30d,
} from "@/services/permit-service";
import { getWo } from "@/services/wo-service";
import { PERMIT_TYPES_CATALOG_CODE } from "@/stores/reference-manager-store";
import { P } from "@shared/rbac/permissions.generated";

/** Every permit status the lifecycle can produce (filter options, in lifecycle order). */
export const PERMIT_STATUSES = [
  "draft",
  "pending_review",
  "approved",
  "issued",
  "active",
  "suspended",
  "revalidation_required",
  "handed_back",
  "closed",
  "cancelled",
  "expired",
] as const;

/** Rows fetched per load; "load more" raises it up to the backend maximum. */
const PAGE_SIZE = 200;
const BACKEND_MAX = 2000;

export function PermitsPage() {
  const { t } = useTranslation("permits");
  const typeName = usePermitTypeName();
  const { formatDateTime, formatNumber, formatPercent } = useFormatters();
  const [searchParams, setSearchParams] = useSearchParams();
  const { can } = usePermissions();
  // Permit types are plant-wide: the issue right counts at no node only.
  const plantWide = usePermitPlantWideAccess();

  useEffect(() => {
    void i18n.loadNamespaces([...PERMIT_REFUSAL_NAMESPACES]);
  }, []);

  const [types, setTypes] = useState<PermitTypeRecord[]>([]);
  const [permits, setPermits] = useState<WorkPermitRecord[]>([]);
  const [openCount, setOpenCount] = useState<number | null>(null);
  /** Permits the list / the open report leave out: outside the user's sites, or without a site. */
  const [hidden, setHidden] = useState({ outsideScope: 0, withoutSite: 0 });
  const [openHidden, setOpenHidden] = useState(0);
  const [kpi30d, setKpi30d] = useState<PermitComplianceKpi30dRecord | null>(null);
  const [kpiError, setKpiError] = useState<unknown>(null);
  const [kpiLoading, setKpiLoading] = useState(true);
  /** A permit opened by id (deep link) that is not in the loaded page. */
  const [linkedPermit, setLinkedPermit] = useState<WorkPermitRecord | null>(null);
  const [linkError, setLinkError] = useState<unknown>(null);
  const [linkNonce, setLinkNonce] = useState(0);
  const [limit, setLimit] = useState(PAGE_SIZE);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [typesOpen, setTypesOpen] = useState(false);
  const [createWo, setCreateWo] = useState<PermitCreateWorkOrder | null>(null);
  const [showFilters, setShowFilters] = useState(true);
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);

  const loadList = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      // One extra row tells whether more permits exist beyond the current limit.
      const term = search.trim();
      const [ty, page] = await Promise.all([
        listPermitTypes(),
        listWorkPermits({
          limit: Math.min(limit + 1, BACKEND_MAX),
          ...(statusFilter ? { status: statusFilter } : {}),
          ...(typeFilter ? { permit_type_id: Number(typeFilter) } : {}),
          ...(term ? { search: term } : {}),
        }),
      ]);
      setTypes(ty);
      const rows = page.items;
      setHidden({
        outsideScope: page.hidden_outside_scope,
        withoutSite: page.hidden_without_site,
      });
      setHasMore(rows.length > limit);
      setPermits(rows.slice(0, limit));
    } catch (e) {
      setLoadError(e);
    } finally {
      setLoading(false);
    }
  }, [limit, statusFilter, typeFilter, search]);

  const loadKpis = useCallback(async () => {
    setKpiLoading(true);
    setKpiError(null);
    try {
      const [kpi, open] = await Promise.all([permitComplianceKpi30d(), listOpenPermitsReport()]);
      setKpi30d(kpi);
      setOpenCount(open.items.length);
      setOpenHidden(open.hidden_outside_scope + open.hidden_without_site);
    } catch (e) {
      setKpiError(e);
      setKpi30d(null);
      setOpenCount(null);
      setOpenHidden(0);
    } finally {
      setKpiLoading(false);
    }
  }, []);

  const load = useCallback(async () => {
    await Promise.all([loadList(), loadKpis()]);
  }, [loadList, loadKpis]);

  useEffect(() => {
    void loadList();
  }, [loadList]);

  useEffect(() => {
    void loadKpis();
  }, [loadKpis]);

  // Deep links: `?permit=<id>` opens a permit; `?createForWo=<id>` opens "create" for that OT.
  useEffect(() => {
    const selectParam = searchParams.get(PERMIT_QUERY_SELECT);
    const woParam = searchParams.get(PERMIT_QUERY_CREATE_FOR_WO);
    if (selectParam) {
      const id = Number(selectParam);
      if (Number.isFinite(id) && id > 0) setSelectedId(id);
    }
    if (!woParam) return;
    let cancelled = false;
    void getWo(Number(woParam))
      .then(({ wo }) => {
        if (cancelled) return;
        const assetLabel =
          wo.asset_code && wo.asset_label
            ? `${wo.asset_code} — ${wo.asset_label}`
            : (wo.asset_code ?? wo.asset_label ?? null);
        setCreateWo({ id: wo.id, code: wo.code, assetLabel, siteId: wo.entity_id });
        setCreateOpen(true);
      })
      .catch(() => {
        // The global IPC toast explains why the OT could not be loaded.
      });
    return () => {
      cancelled = true;
    };
  }, [searchParams]);

  const clearParam = useCallback(
    (key: string) => {
      if (!searchParams.has(key)) return;
      const next = new URLSearchParams(searchParams);
      next.delete(key);
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams],
  );

  const inPage = useMemo(
    () => permits.find((p) => p.id === selectedId) ?? null,
    [permits, selectedId],
  );

  // A permit outside the loaded page (deep link, old permit): read it by id.
  useEffect(() => {
    if (selectedId == null || inPage != null) {
      setLinkError(null);
      return;
    }
    if (linkedPermit?.id === selectedId) return;
    let cancelled = false;
    setLinkError(null);
    getWorkPermit(selectedId)
      .then((row) => {
        if (!cancelled) setLinkedPermit(row);
      })
      .catch((e: unknown) => {
        if (!cancelled) setLinkError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId, inPage, linkedPermit?.id, linkNonce]);

  const selected = inPage ?? (linkedPermit?.id === selectedId ? linkedPermit : null);
  const detailOpen = selectedId != null && (selected != null || linkError != null);

  const handleChanged = (updated: WorkPermitRecord) => {
    setPermits((rows) => rows.map((r) => (r.id === updated.id ? updated : r)));
    if (linkedPermit?.id === updated.id) setLinkedPermit(updated);
    void load();
  };

  const statusLabel = useCallback(
    (code: string) => t(`status.${code}`, { defaultValue: t("status.unknown") }),
    [t],
  );

  const columns = useMemo<ColumnDef<WorkPermitRecord>[]>(
    () => [
      {
        accessorKey: "code",
        header: t("columns.code"),
        cell: ({ row }) => (
          // Keyboard path to the detail (DataTable rows are pointer-only).
          <button
            type="button"
            className="min-h-[36px] rounded font-mono text-xs font-semibold text-text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("actions.select", { code: row.original.code })}
            onClick={(e) => {
              e.stopPropagation();
              setSelectedId(row.original.id);
            }}
          >
            {row.original.code}
          </button>
        ),
      },
      {
        id: "status",
        accessorFn: (row) => statusLabel(row.status),
        header: t("columns.status"),
        cell: ({ row }) => (
          <span className="flex flex-wrap items-center gap-1.5">
            <StatusChip
              label={statusLabel(row.original.status)}
              tone={toneOf(PERMIT_STATUS_TONE, row.original.status)}
              dot
            />
            <PermitNoSiteBadge permit={row.original} />
          </span>
        ),
      },
      {
        id: "type",
        accessorFn: (row) => typeName(row.permit_type_code, row.permit_type_name),
        header: t("columns.type"),
        cell: ({ row }) => (
          <span className="text-xs">
            {row.original.permit_type_name == null
              ? t("detail.none")
              : typeName(row.original.permit_type_code, row.original.permit_type_name)}
          </span>
        ),
      },
      {
        id: "asset",
        accessorFn: (row) => formatAssetLabel(row) ?? "",
        header: t("columns.asset"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {formatAssetLabel(row.original) ?? t("detail.none")}
          </span>
        ),
      },
      {
        id: "workOrder",
        accessorFn: (row) => row.linked_work_order_code ?? "",
        header: t("columns.workOrder"),
        cell: ({ row }) =>
          row.original.linked_work_order_code ? (
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.linked_work_order_code}
              entityId={row.original.linked_work_order_id}
            />
          ) : (
            <span className="text-xs text-text-muted">{t("detail.none")}</span>
          ),
      },
      {
        id: "expires",
        accessorFn: (row) => row.expires_at ?? "",
        header: t("columns.expires"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.expires_at ? formatDateTime(row.original.expires_at) : t("detail.none")}
          </span>
        ),
      },
    ],
    [t, statusLabel, formatDateTime, typeName],
  );

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "status",
        kind: "select",
        label: t("filters.status"),
        options: PERMIT_STATUSES.map((code) => ({ value: code, label: statusLabel(code) })),
        value: statusFilter,
        onChange: (v) => {
          setStatusFilter(v);
          setLimit(PAGE_SIZE);
        },
      },
      {
        id: "type",
        kind: "select",
        label: t("filters.type"),
        options: types.map((ty) => ({ value: String(ty.id), label: typeName(ty.code, ty.name) })),
        value: typeFilter,
        onChange: (v) => {
          setTypeFilter(v);
          setLimit(PAGE_SIZE);
        },
      },
    ],
    [t, statusLabel, statusFilter, typeFilter, types, typeName],
  );

  const resetFilters = () => {
    setSearchInput("");
    setSearch("");
    setStatusFilter(null);
    setTypeFilter(null);
    setLimit(PAGE_SIZE);
  };

  const activeFilterCount = [statusFilter, typeFilter].filter(Boolean).length;
  const showLoadError = loadError != null && permits.length === 0;

  return (
    <ModulePageShell
      icon={HardHat}
      title={t("page.title")}
      count={permits.length}
      actions={
        <>
          <PermissionGate permission={P.PTW_ISSUE}>
            <Button
              className={mfLayout.primaryAction}
              onClick={() => {
                setCreateWo(null);
                setCreateOpen(true);
              }}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("actions.newPermit")}
            </Button>
          </PermissionGate>
          {/* Permit types live in Données de référence; a permit issuer without access to it
              edits them here, in the same editor. */}
          {can(P.REF_VIEW) ? (
            <Button asChild variant="outline" className="h-11 gap-1.5">
              <Link to={listLinks.referenceCatalog(PERMIT_TYPES_CATALOG_CODE)}>
                <Settings2 className="h-4 w-4" aria-hidden />
                {t("actions.manageTypes")}
              </Link>
            </Button>
          ) : plantWide.can(P.PTW_ISSUE) ? (
            <Button variant="outline" className="h-11 gap-1.5" onClick={() => setTypesOpen(true)}>
              <Settings2 className="h-4 w-4" aria-hidden />
              {t("actions.manageTypes")}
            </Button>
          ) : null}
          <FilterToggleButton
            open={showFilters}
            onToggle={() => setShowFilters((v) => !v)}
            activeCount={activeFilterCount}
          />
          <RefreshButton onRefresh={() => void load()} loading={loading} />
        </>
      }
      filterBar={
        <>
          {showFilters ? (
            <SmartFilterBar
              searchPlaceholder={t("filters.searchPlaceholder")}
              searchValue={searchInput}
              onSearchInputChange={setSearchInput}
              onSearchChange={setSearch}
              filters={filterDefs}
              resultCount={permits.length}
              onReset={resetFilters}
            />
          ) : null}
          <PermitHiddenNotice
            outsideScope={hidden.outsideScope}
            withoutSite={hidden.withoutSite}
            className="mx-6 mt-3"
          />
          {/* Forced lock removals whose worker was not told yet (permit close-right holders). */}
          {can(P.PTW_CLOSE) ? (
            <ForcedRemovalReviewList
              onOpenPermit={setSelectedId}
              reloadKey={permits}
              className="mx-6 mt-3"
            />
          ) : null}
          <div className="px-6 py-3">
            {kpiError != null ? (
              <ErrorState
                error={kpiError}
                title={t("kpi.loadError")}
                onRetry={() => void loadKpis()}
                retrying={kpiLoading}
                className="py-4"
              />
            ) : kpiLoading && kpi30d == null ? (
              <LoadingState />
            ) : (
              <div className="grid gap-3 sm:grid-cols-3" data-testid="permits-kpis">
                <KpiTile
                  label={t("kpi.open")}
                  value={openCount != null ? formatNumber(openCount) : "—"}
                  hint={
                    openHidden > 0
                      ? `${t("kpi.openHint")} · ${t("siteScope.openHidden", { count: openHidden })}`
                      : t("kpi.openHint")
                  }
                  icon={HardHat}
                />
                <KpiTile
                  label={t("kpi.activated")}
                  value={kpi30d ? formatNumber(kpi30d.activated_count) : "—"}
                  hint={t("kpi.period30d")}
                  icon={CalendarCheck}
                />
                <KpiTile
                  label={t("kpi.onTimeRate")}
                  value={kpi30d?.rate != null ? formatPercent(kpi30d.rate) : "—"}
                  hint={
                    kpi30d
                      ? t("kpi.onTimeHint", {
                          onTime: formatNumber(kpi30d.handed_back_on_time_count),
                          activated: formatNumber(kpi30d.activated_count),
                        })
                      : t("kpi.period30d")
                  }
                  icon={ShieldCheck}
                />
              </div>
            )}
          </div>
        </>
      }
    >
      {showLoadError ? (
        <ErrorState error={loadError} onRetry={() => void load()} retrying={loading} />
      ) : (
        <div className="space-y-3">
          {hasMore ? (
            <div
              role="status"
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-xs text-tone-info-fg"
            >
              <span>{t("list.moreAvailable", { shown: permits.length })}</span>
              {limit < BACKEND_MAX ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-10"
                  disabled={loading}
                  onClick={() => setLimit((l) => Math.min(l + PAGE_SIZE, BACKEND_MAX))}
                >
                  {t("list.loadMore")}
                </Button>
              ) : null}
            </div>
          ) : null}
          <DataTable
            columns={columns}
            data={permits}
            pageSize={20}
            isLoading={loading && permits.length === 0}
            skeletonRows={6}
            onRowClick={(row) => setSelectedId(row.id)}
            isRowSelected={(row) => row.id === selectedId}
          />
          {!loading && permits.length === 0 ? (
            <p className="text-sm text-text-muted">
              {search.trim() || statusFilter || typeFilter ? t("list.noMatch") : t("list.empty")}
            </p>
          ) : null}
        </div>
      )}

      <Dialog
        open={detailOpen}
        onOpenChange={(open) => {
          if (open) return;
          setSelectedId(null);
          setLinkError(null);
          clearParam(PERMIT_QUERY_SELECT);
        }}
      >
        <DialogContent className="flex max-h-[90vh] w-full max-w-4xl flex-col gap-0 p-0">
          <DialogHeader className="px-6 pt-5 pb-3">
            <DialogTitle>
              {selected ? t("detail.title", { code: selected.code }) : t("page.title")}
            </DialogTitle>
            <DialogDescription>{t("detail.dialogDescription")}</DialogDescription>
          </DialogHeader>
          <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-6">
            {selected ? (
              <PermitDetailPanel permit={selected} onChanged={handleChanged} />
            ) : linkError != null ? (
              <ErrorState
                error={linkError}
                title={t("detail.linkError")}
                onRetry={() => {
                  setLinkedPermit(null);
                  setLinkNonce((n) => n + 1);
                }}
              />
            ) : null}
          </div>
        </DialogContent>
      </Dialog>

      <PermitCreateDialog
        open={createOpen}
        onOpenChange={(open) => {
          setCreateOpen(open);
          if (!open) clearParam(PERMIT_QUERY_CREATE_FOR_WO);
        }}
        types={types}
        workOrder={createWo}
        onCreated={(created) => {
          setPermits((rows) => [created, ...rows]);
          setSelectedId(created.id);
          void load();
        }}
      />
      <Dialog open={typesOpen} onOpenChange={setTypesOpen}>
        <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("types.title")}</DialogTitle>
          </DialogHeader>
          <PermitTypesHost embedded onSaved={() => void load()} />
        </DialogContent>
      </Dialog>
    </ModulePageShell>
  );
}

import type { TFunction } from "i18next";
import { AlertTriangle, Archive, ArrowLeft, FolderTree, ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { ArchiveReasonDialog } from "@/components/archive/ArchiveReasonDialog";
import {
  archiveActionLabel,
  archiveClassLabel,
  archiveModuleLabel,
  archiveRecordName,
  fileSafe,
  purgeModeLabel,
  restorePolicyLabel,
} from "@/components/archive/archive-labels";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Timeline } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ARCHIVE_ACTION_RESULT_TONE, toneOf } from "@/design-system/tone";
import { saveBlobWithDialog } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type ArchiveItemDetail,
  type ArchiveItemSummary,
  exportArchiveItems,
  getArchiveItem,
  listArchiveItems,
  purgeArchiveItems,
  restoreArchiveItem,
  setLegalHold,
} from "@/services/archive-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

interface ArchiveExplorerProps {
  className?: string;
}

const LIST_PAGE_SIZE = 25;
/** Rows fetched per query; when reached, the notice asks to narrow the filters. */
const FETCH_LIMIT = 500;

type BulkAction = "legalHold" | "purge";

function reportFailure(err: unknown) {
  const { title, description } = describeAppError(err);
  pushAppToast({ title, ...(description ? { description } : {}), variant: "destructive" });
}

/** Writes archive export JSON through the native Save dialog. Returns false if cancelled. */
async function saveJson(fileName: string, payload: unknown, dialogTitle: string): Promise<boolean> {
  const blob = new Blob([JSON.stringify(payload, null, 2)], {
    type: "application/json;charset=utf-8",
  });
  try {
    await saveBlobWithDialog(blob, fileName, {
      title: dialogTitle,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    return true;
  } catch (err) {
    if (isExportCancelled(err)) return false;
    throw err;
  }
}

export function ArchiveExplorer({ className }: ArchiveExplorerProps) {
  const { t } = useTranslation("archive");
  const { formatDate, formatNumber } = useFormatters();
  const { can, isLoading: permissionsLoading } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [items, setItems] = useState<ArchiveItemSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [page, setPage] = useState(0);

  const [sourceModule, setSourceModule] = useState<string | undefined>(undefined);
  const [searchText, setSearchText] = useState("");
  // The list reloads when typing pauses, not on every keystroke.
  const debouncedSearch = useDebouncedValue(searchText, 350);
  // Open levels of the Module > Class > Year tree (clicking a level also sets its filter).
  const [openNodes, setOpenNodes] = useState<Set<string>>(new Set());
  const [legalHoldOnly, setLegalHoldOnly] = useState(false);
  const [selectedClasses, setSelectedClasses] = useState<Set<string>>(new Set());
  const [dateFrom, setDateFrom] = useState<string>("");
  const [dateTo, setDateTo] = useState<string>("");
  const [bulkAction, setBulkAction] = useState<BulkAction | null>(null);
  const [exporting, setExporting] = useState(false);

  const loadItems = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const filter: {
        source_module?: string;
        legal_hold?: boolean;
        search_text?: string;
        date_from?: string;
        date_to?: string;
        limit: number;
        offset: number;
      } = { limit: FETCH_LIMIT, offset: 0 };
      if (sourceModule) filter.source_module = sourceModule;
      if (legalHoldOnly) filter.legal_hold = true;
      if (debouncedSearch.trim()) filter.search_text = debouncedSearch.trim();
      if (dateFrom) filter.date_from = dateFrom;
      if (dateTo) filter.date_to = `${dateTo}T23:59:59Z`;

      const data = await listArchiveItems(filter);
      setItems(data);
      setPage(0);
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, [dateFrom, dateTo, legalHoldOnly, debouncedSearch, sourceModule]);

  useEffect(() => {
    void loadItems();
  }, [loadItems]);

  const moduleClassYearTree = useMemo(() => {
    const tree = new Map<string, Map<string, Map<string, number>>>();
    for (const item of items) {
      const moduleMap = tree.get(item.source_module) ?? new Map<string, Map<string, number>>();
      const classMap = moduleMap.get(item.archive_class) ?? new Map<string, number>();
      const year = new Date(item.archived_at).getUTCFullYear();
      const yearKey = Number.isFinite(year) ? String(year) : "";
      classMap.set(yearKey, (classMap.get(yearKey) ?? 0) + 1);
      moduleMap.set(item.archive_class, classMap);
      tree.set(item.source_module, moduleMap);
    }
    return tree;
  }, [items]);

  const availableClasses = useMemo(
    () => Array.from(new Set(items.map((i) => i.archive_class))).sort(),
    [items],
  );

  const filteredItems = useMemo(() => {
    if (selectedClasses.size === 0) return items;
    return items.filter((item) => selectedClasses.has(item.archive_class));
  }, [items, selectedClasses]);

  const visibleItems = useMemo(() => {
    const start = page * LIST_PAGE_SIZE;
    return filteredItems.slice(start, start + LIST_PAGE_SIZE);
  }, [filteredItems, page]);
  const hasMore = (page + 1) * LIST_PAGE_SIZE < filteredItems.length;

  const stats = useMemo(() => {
    const legalHoldCount = filteredItems.filter((i) => i.legal_hold).length;
    return {
      total: filteredItems.length,
      legalHoldCount,
      withoutHold: filteredItems.length - legalHoldCount,
    };
  }, [filteredItems]);

  const toggleNode = useCallback((key: string) => {
    setOpenNodes((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const toggleClassFilter = useCallback((archiveClass: string) => {
    setSelectedClasses((prev) => {
      const next = new Set(prev);
      if (next.has(archiveClass)) next.delete(archiveClass);
      else next.add(archiveClass);
      return next;
    });
    setPage(0);
  }, []);

  const toggleSelectedRow = useCallback((archiveItemId: number, checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (checked) next.add(archiveItemId);
      else next.delete(archiveItemId);
      return next;
    });
  }, []);

  const selectedRows = useMemo(
    () => filteredItems.filter((item) => selectedIds.has(item.id)),
    [filteredItems, selectedIds],
  );

  const allVisibleSelected =
    visibleItems.length > 0 && visibleItems.every((item) => selectedIds.has(item.id));
  const toggleSelectAll = useCallback(
    (checked: boolean) => {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        for (const item of visibleItems) {
          if (checked) next.add(item.id);
          else next.delete(item.id);
        }
        return next;
      });
    },
    [visibleItems],
  );

  const clearBulkSelection = useCallback(() => setSelectedIds(new Set()), []);

  const runBulkExport = useCallback(async () => {
    if (selectedRows.length === 0) return;
    setExporting(true);
    try {
      const payload = await exportArchiveItems({
        archive_item_ids: selectedRows.map((r) => r.id),
        export_reason: t("export.bulkReason"),
      });
      const date = new Date().toISOString().slice(0, 10);
      const saved = await saveJson(`archive-export-${date}.json`, payload, t("export.dialogTitle"));
      if (saved) {
        pushAppToast({
          title: t("export.saved", { count: selectedRows.length }),
          variant: "success",
        });
      }
    } catch (err) {
      reportFailure(err);
    } finally {
      setExporting(false);
    }
  }, [selectedRows, t]);

  const runBulk = useCallback(
    async (action: BulkAction, reason: string) => {
      if (action === "legalHold") {
        for (const row of selectedRows) {
          await withStepUp(() =>
            setLegalHold({ archive_item_id: row.id, enable: true, reason_note: reason }),
          );
        }
        pushAppToast({
          title: t("bulk.legalHoldDone", { count: selectedRows.length }),
          variant: "success",
        });
      } else {
        const result = await withStepUp(() =>
          purgeArchiveItems({
            archive_item_ids: selectedRows.map((row) => row.id),
            purge_reason: reason,
          }),
        );
        pushAppToast({
          title: t("bulk.purgeDone", {
            purged: result.purged_item_ids.length,
            blocked: result.blocked_items.length,
          }),
          ...(result.blocked_items.length > 0
            ? {
                description: result.strict_mode
                  ? t("bulk.purgeBlockedStrict")
                  : t("bulk.purgeBlocked"),
              }
            : {}),
          variant: result.blocked_items.length > 0 ? "default" : "success",
        });
      }
      clearBulkSelection();
      await loadItems();
    },
    [clearBulkSelection, loadItems, selectedRows, t, withStepUp],
  );

  const resetFilters = () => {
    setSourceModule(undefined);
    setSearchText("");
    setLegalHoldOnly(false);
    setSelectedClasses(new Set());
    setDateFrom("");
    setDateTo("");
    setPage(0);
  };

  return (
    <div className={className}>
      {StepUpDialogElement}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[300px_minmax(0,1fr)]">
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">{t("explorer.filtersTitle")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="rounded-md border border-surface-border p-3">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium">
                <FolderTree className="h-4 w-4" aria-hidden />
                {t("explorer.tree")}
              </div>
              <div
                className="max-h-60 overflow-auto pr-1 text-sm"
                data-testid="archive-folder-tree"
              >
                {moduleClassYearTree.size === 0 ? (
                  <p className="text-xs text-text-muted">{t("explorer.treeEmpty")}</p>
                ) : null}
                {Array.from(moduleClassYearTree.entries())
                  .sort(([a], [b]) => a.localeCompare(b))
                  .map(([module, classMap]) => (
                    <details key={module} className="mb-1" open={openNodes.has(module)}>
                      <summary
                        className="cursor-pointer rounded px-1 py-0.5 hover:bg-surface-2"
                        onClick={(e) => {
                          e.preventDefault();
                          toggleNode(module);
                          setSourceModule((prev) => (prev === module ? undefined : module));
                        }}
                      >
                        <span className={sourceModule === module ? "font-semibold" : undefined}>
                          {archiveModuleLabel(t, module)}
                        </span>
                      </summary>
                      <div className="ml-3 mt-1 space-y-1 border-l border-surface-border pl-2">
                        {Array.from(classMap.entries())
                          .sort(([a], [b]) => a.localeCompare(b))
                          .map(([archiveClass, yearMap]) => (
                            <details
                              key={`${module}-${archiveClass}`}
                              open={openNodes.has(`${module}/${archiveClass}`)}
                            >
                              <summary
                                className="cursor-pointer rounded px-1 py-0.5 hover:bg-surface-2"
                                onClick={(event) => {
                                  event.preventDefault();
                                  toggleNode(`${module}/${archiveClass}`);
                                  setSelectedClasses(new Set([archiveClass]));
                                  setPage(0);
                                }}
                              >
                                <span className="text-left">
                                  {archiveClassLabel(t, archiveClass)}
                                </span>
                              </summary>
                              <div className="ml-3 mt-1 space-y-1 border-l border-surface-border pl-2">
                                {Array.from(yearMap.entries())
                                  .sort(([a], [b]) => b.localeCompare(a))
                                  .map(([year, n]) => (
                                    <button
                                      key={`${module}-${archiveClass}-${year}`}
                                      type="button"
                                      className="flex w-full items-center justify-between rounded px-1 py-0.5 text-left hover:bg-surface-2"
                                      disabled={!year}
                                      onClick={() => {
                                        setSelectedClasses(new Set([archiveClass]));
                                        setDateFrom(`${year}-01-01`);
                                        setDateTo(`${year}-12-31`);
                                        setPage(0);
                                      }}
                                    >
                                      <span>{year || t("explorer.unknownYear")}</span>
                                      <span className="text-[10px] text-text-muted"> {n}</span>
                                    </button>
                                  ))}
                              </div>
                            </details>
                          ))}
                      </div>
                    </details>
                  ))}
              </div>
            </div>

            <div className="space-y-2">
              <Input
                value={searchText}
                onChange={(e) => setSearchText(e.target.value)}
                placeholder={t("explorer.searchPlaceholder")}
                aria-label={t("explorer.searchPlaceholder")}
              />
              <div className="flex items-center gap-2 text-sm">
                <Checkbox
                  id="archive-legal-hold-only"
                  checked={legalHoldOnly}
                  onCheckedChange={(checked) => setLegalHoldOnly(checked)}
                />
                <label htmlFor="archive-legal-hold-only" className="cursor-pointer">
                  {t("explorer.legalHoldOnly")}
                </label>
              </div>
              {availableClasses.length > 0 ? (
                <div className="space-y-1">
                  <p className="text-xs text-text-muted">{t("explorer.classFilters")}</p>
                  <div className="flex flex-wrap gap-1">
                    {availableClasses.map((archiveClass) => {
                      const active = selectedClasses.has(archiveClass);
                      return (
                        <button
                          key={archiveClass}
                          type="button"
                          aria-pressed={active}
                          className={cn(
                            "min-h-[32px] rounded-full border px-2 py-0.5 text-xs",
                            active
                              ? "border-primary bg-primary/10 text-primary"
                              : "border-surface-border",
                          )}
                          onClick={() => toggleClassFilter(archiveClass)}
                        >
                          {archiveClassLabel(t, archiveClass)}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}
              <div className="grid grid-cols-2 gap-2">
                <label className="flex flex-col gap-1 text-xs text-text-muted">
                  {t("explorer.dateFrom")}
                  <Input
                    type="date"
                    value={dateFrom}
                    onChange={(e) => setDateFrom(e.target.value)}
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-text-muted">
                  {t("explorer.dateTo")}
                  <Input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
                </label>
              </div>
              <div className="flex gap-2">
                <Button size="sm" variant="outline" className="h-10" onClick={resetFilters}>
                  {t("explorer.reset")}
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">{t("explorer.recordsTitle")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {selectedRows.length > 0 && !permissionsLoading && !selectedId ? (
              <div className="flex flex-wrap items-center gap-2 rounded-md border border-surface-border bg-surface-2 p-2">
                <span className="text-sm text-text-muted">
                  {t("bulk.selected", { count: selectedRows.length })}
                </span>
                {can(P.ARC_EXPORT) ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={exporting}
                    onClick={() => void runBulkExport()}
                  >
                    {t("bulk.export")}
                  </Button>
                ) : null}
                {can(P.ARC_PURGE) ? (
                  <Button size="sm" variant="outline" onClick={() => setBulkAction("legalHold")}>
                    {t("bulk.legalHold")}
                  </Button>
                ) : null}
                {can(P.ARC_PURGE) ? (
                  <Button size="sm" variant="destructive" onClick={() => setBulkAction("purge")}>
                    {t("bulk.purge")}
                  </Button>
                ) : null}
                <Button size="sm" variant="ghost" onClick={clearBulkSelection}>
                  {t("bulk.clear")}
                </Button>
              </div>
            ) : null}

            {selectedId !== null ? (
              <ArchiveItemDetailView
                itemId={selectedId}
                onBack={() => setSelectedId(null)}
                onChanged={loadItems}
                withStepUp={withStepUp}
                canExport={can(P.ARC_EXPORT)}
                canRestore={can(P.ARC_RESTORE)}
                canPurge={can(P.ARC_PURGE)}
              />
            ) : loadError ? (
              <ErrorState
                error={loadError}
                title={t("explorer.loadError")}
                onRetry={() => void loadItems()}
              />
            ) : loading && items.length === 0 ? (
              <LoadingState label={t("explorer.loading")} />
            ) : (
              <>
                <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                  <KpiTile
                    label={t("stats.total")}
                    value={formatNumber(stats.total)}
                    icon={Archive}
                  />
                  <KpiTile
                    label={t("stats.legalHold")}
                    value={formatNumber(stats.legalHoldCount)}
                    icon={ShieldAlert}
                    {...(stats.legalHoldCount > 0 ? { tone: "attention" as const } : {})}
                  />
                  <KpiTile
                    label={t("stats.withoutHold")}
                    value={formatNumber(stats.withoutHold)}
                    hint={t("stats.withoutHoldHint")}
                    icon={AlertTriangle}
                  />
                </div>

                {items.length >= FETCH_LIMIT ? (
                  <p
                    role="status"
                    className="rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-xs text-tone-info-fg"
                  >
                    {t("explorer.limitReached", { count: FETCH_LIMIT })}
                  </p>
                ) : null}

                {filteredItems.length === 0 ? (
                  <EmptyState
                    icon={<Archive className="h-6 w-6" aria-hidden />}
                    title={t("explorer.empty")}
                    description={t("explorer.emptyHint")}
                  />
                ) : (
                  <div className="rounded-md border border-surface-border">
                    <div className="grid grid-cols-[36px_1fr_auto_auto_auto] gap-2 border-b border-surface-border bg-surface-2 px-3 py-2 text-xs font-medium text-text-muted">
                      <Checkbox
                        checked={allVisibleSelected}
                        onCheckedChange={(checked) => toggleSelectAll(checked)}
                        aria-label={t("explorer.selectAll")}
                      />
                      <span>{t("explorer.col.record")}</span>
                      <span>{t("explorer.col.class")}</span>
                      <span>{t("explorer.col.status")}</span>
                      <span>{t("explorer.col.archived")}</span>
                    </div>
                    <ul className="max-h-[540px] overflow-auto">
                      {visibleItems.map((item) => {
                        const name = archiveRecordName(t, item);
                        return (
                          <li
                            key={item.id}
                            className="grid grid-cols-[36px_1fr_auto_auto_auto] items-center gap-2 border-b border-surface-border px-3 py-1.5 text-sm"
                          >
                            <Checkbox
                              checked={selectedIds.has(item.id)}
                              onCheckedChange={(checked) => toggleSelectedRow(item.id, checked)}
                              aria-label={t("explorer.selectRecord", { name })}
                            />
                            <button
                              type="button"
                              className="min-h-[36px] min-w-0 rounded text-left hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                              onClick={() => setSelectedId(item.id)}
                            >
                              <span className="block truncate font-medium">{name}</span>
                              <span className="block truncate text-xs text-text-muted">
                                {archiveModuleLabel(t, item.source_module)}
                              </span>
                            </button>
                            <StatusChip
                              label={archiveClassLabel(t, item.archive_class)}
                              tone="neutral"
                            />
                            {item.legal_hold ? (
                              <StatusChip label={t("status.legalHold")} tone="attention" />
                            ) : (
                              <StatusChip label={t("status.normal")} tone="neutral" />
                            )}
                            <span className="whitespace-nowrap text-xs text-text-muted">
                              {formatDate(item.archived_at)}
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                  </div>
                )}

                {filteredItems.length > LIST_PAGE_SIZE ? (
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-text-muted">
                      {t("explorer.showing", {
                        from: page * LIST_PAGE_SIZE + 1,
                        to: page * LIST_PAGE_SIZE + visibleItems.length,
                        total: filteredItems.length,
                      })}
                    </span>
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={page === 0}
                        onClick={() => setPage((prev) => Math.max(0, prev - 1))}
                      >
                        {t("explorer.previous")}
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={!hasMore}
                        onClick={() => setPage((prev) => prev + 1)}
                      >
                        {t("explorer.next")}
                      </Button>
                    </div>
                  </div>
                ) : null}
              </>
            )}
          </CardContent>
        </Card>
      </div>

      <ArchiveReasonDialog
        open={bulkAction !== null}
        onOpenChange={(open) => {
          if (!open) setBulkAction(null);
        }}
        title={bulkAction === "purge" ? t("bulk.purgeTitle") : t("bulk.legalHoldTitle")}
        description={
          bulkAction === "purge"
            ? t("bulk.purgeDescription", { count: selectedRows.length })
            : t("bulk.legalHoldDescription", { count: selectedRows.length })
        }
        confirmLabel={
          bulkAction === "purge"
            ? t("bulk.purgeConfirm", { count: selectedRows.length })
            : t("bulk.legalHoldConfirm", { count: selectedRows.length })
        }
        destructive={bulkAction === "purge"}
        onConfirm={async (reason) => {
          if (bulkAction) await runBulk(bulkAction, reason);
        }}
      />
    </div>
  );
}

type ItemAction = "restore" | "legalHold" | "purge";

interface ArchiveItemDetailViewProps {
  itemId: number;
  canExport: boolean;
  canRestore: boolean;
  canPurge: boolean;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  onBack: () => void;
  onChanged: () => Promise<void>;
}

function ArchiveItemDetailView({
  itemId,
  canExport,
  canRestore,
  canPurge,
  withStepUp,
  onBack,
  onChanged,
}: ArchiveItemDetailViewProps) {
  const { t } = useTranslation("archive");
  const { formatDate, formatDateTime, locale } = useFormatters();
  const [detail, setDetail] = useState<ArchiveItemDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [tab, setTab] = useState<"record" | "history">("record");
  const [action, setAction] = useState<ItemAction | null>(null);
  const [exporting, setExporting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setDetail(await getArchiveItem(itemId));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [itemId]);

  useEffect(() => {
    void load();
  }, [load]);

  const back = (
    <Button size="sm" variant="outline" className="h-10" onClick={onBack}>
      <ArrowLeft className="mr-1.5 h-4 w-4" aria-hidden />
      {t("detail.back")}
    </Button>
  );

  if (loading && !detail) return <LoadingState label={t("detail.loading")} />;
  if (error || !detail) {
    return (
      <div className="space-y-3">
        {back}
        <ErrorState error={error} title={t("detail.loadError")} onRetry={() => void load()} />
      </div>
    );
  }

  const { item } = detail;
  const name = archiveRecordName(t, item, detail.payload?.payload_json);
  const purgeCheck = evaluatePurgeEligibility(detail, t, formatDate);
  const restoreBlockedReason =
    item.restore_policy === "not_allowed"
      ? t("detail.restoreBlocked.policy")
      : item.legal_hold
        ? t("detail.restoreBlocked.legalHold")
        : detail.retention_policy && !detail.retention_policy.allow_restore
          ? t("detail.restoreBlocked.retention")
          : null;

  const afterChange = async () => {
    await onChanged();
    await load();
  };

  const exportOne = async () => {
    setExporting(true);
    try {
      const payload = await exportArchiveItems({
        archive_item_ids: [item.id],
        export_reason: t("export.singleReason"),
      });
      const saved = await saveJson(
        `archive-${fileSafe(name)}.json`,
        payload.items[0]?.payload_json ?? payload,
        t("export.dialogTitle"),
      );
      if (saved) pushAppToast({ title: t("export.saved", { count: 1 }), variant: "success" });
    } catch (err) {
      reportFailure(err);
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">{back}</div>

      <div className="rounded-md border border-surface-border p-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium text-text-primary">{name}</span>
          <StatusChip label={archiveClassLabel(t, item.archive_class)} tone="neutral" />
          {item.legal_hold ? <StatusChip label={t("status.legalHold")} tone="attention" /> : null}
          <StatusChip
            label={detail.checksum_valid ? t("detail.checksumOk") : t("detail.checksumBad")}
            tone={detail.checksum_valid ? "success" : "danger"}
          />
        </div>
        <dl className="mt-2 grid grid-cols-1 gap-x-4 gap-y-1 text-sm md:grid-cols-2">
          <div>
            <dt className="inline text-text-muted">{t("detail.module")} </dt>
            <dd className="inline">{archiveModuleLabel(t, item.source_module)}</dd>
          </div>
          <div>
            <dt className="inline text-text-muted">{t("detail.archivedAt")} </dt>
            <dd className="inline">{formatDateTime(item.archived_at)}</dd>
          </div>
          <div>
            <dt className="inline text-text-muted">{t("detail.restorePolicy")} </dt>
            <dd className="inline">{restorePolicyLabel(t, item.restore_policy)}</dd>
          </div>
          <div>
            <dt className="inline text-text-muted">{t("detail.retention")} </dt>
            <dd className="inline">
              {detail.retention_policy
                ? t("detail.retentionValue", {
                    years: detail.retention_policy.retention_years,
                    mode: purgeModeLabel(t, detail.retention_policy.purge_mode),
                  })
                : t("detail.noRetention")}
            </dd>
          </div>
        </dl>
      </div>

      <div className="flex flex-wrap gap-2">
        {canExport ? (
          <Button size="sm" variant="outline" disabled={exporting} onClick={() => void exportOne()}>
            {t("detail.export")}
          </Button>
        ) : null}
        {canRestore ? (
          <Button
            size="sm"
            variant="outline"
            disabled={Boolean(restoreBlockedReason)}
            title={restoreBlockedReason ?? undefined}
            onClick={() => setAction("restore")}
          >
            {t("detail.restore")}
          </Button>
        ) : null}
        {canPurge ? (
          <Button size="sm" variant="outline" onClick={() => setAction("legalHold")}>
            {item.legal_hold ? t("detail.legalHoldOff") : t("detail.legalHoldOn")}
          </Button>
        ) : null}
        {canPurge ? (
          <Button size="sm" variant="destructive" onClick={() => setAction("purge")}>
            {t("detail.purge")}
          </Button>
        ) : null}
      </div>
      {canRestore && restoreBlockedReason ? (
        <p className="text-xs text-text-muted">{restoreBlockedReason}</p>
      ) : null}

      <Tabs value={tab} onValueChange={(value) => setTab(value as "record" | "history")}>
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="record">{t("detail.tabs.record")}</TabsTrigger>
          <TabsTrigger value="history">{t("detail.tabs.history")}</TabsTrigger>
        </TabsList>
      </Tabs>

      {tab === "record" ? (
        <div className="space-y-2 rounded-md border border-surface-border p-3 text-sm">
          <p className="text-text-muted">{t("detail.recordHint")}</p>
          <details>
            <summary className="cursor-pointer select-none text-sm font-medium">
              {t("detail.technicalDetails")}
            </summary>
            <pre className="mt-2 max-h-[320px] overflow-auto rounded bg-surface-2 p-2 text-xs">
              {JSON.stringify(detail.payload?.payload_json ?? null, null, 2)}
            </pre>
          </details>
        </div>
      ) : (
        <Timeline
          items={detail.actions}
          locale={locale}
          aria-label={t("detail.historyAria")}
          empty={<p className="text-sm text-text-muted">{t("detail.historyEmpty")}</p>}
          getEntry={(a) => ({
            id: String(a.id),
            title: archiveActionLabel(t, a.action),
            timestamp: a.action_at,
            badges: [
              {
                label: t(`result.${a.result_status}`, { defaultValue: t("result.other") }),
                tone:
                  toneOf(ARCHIVE_ACTION_RESULT_TONE, a.result_status) === "success"
                    ? "success"
                    : "danger",
              },
            ],
            ...(a.reason_note ? { description: a.reason_note } : {}),
          })}
        />
      )}

      <ArchiveReasonDialog
        open={action === "restore"}
        onOpenChange={(open) => !open && setAction(null)}
        title={t("detail.restoreTitle")}
        description={t("detail.restoreDescription", { name })}
        confirmLabel={t("detail.restoreConfirm")}
        blockedReason={restoreBlockedReason}
        onConfirm={async (reason) => {
          await withStepUp(() =>
            restoreArchiveItem({ archive_item_id: item.id, reason_note: reason }),
          );
          pushAppToast({ title: t("detail.restoreRecorded"), variant: "success" });
          await afterChange();
        }}
      />
      <ArchiveReasonDialog
        open={action === "legalHold"}
        onOpenChange={(open) => !open && setAction(null)}
        title={item.legal_hold ? t("detail.legalHoldOffTitle") : t("detail.legalHoldOnTitle")}
        description={
          item.legal_hold
            ? t("detail.legalHoldOffDescription", { name })
            : t("detail.legalHoldOnDescription", { name })
        }
        confirmLabel={item.legal_hold ? t("detail.legalHoldOff") : t("detail.legalHoldOn")}
        onConfirm={async (reason) => {
          await withStepUp(() =>
            setLegalHold({
              archive_item_id: item.id,
              enable: !item.legal_hold,
              reason_note: reason,
            }),
          );
          await afterChange();
        }}
      />
      <ArchiveReasonDialog
        open={action === "purge"}
        onOpenChange={(open) => !open && setAction(null)}
        title={t("detail.purgeTitle")}
        description={t("detail.purgeDescription", { name })}
        confirmLabel={t("detail.purgeConfirm")}
        destructive
        blockedReason={purgeCheck.eligible ? null : t("detail.purgeNotEligible")}
        onConfirm={async (reason) => {
          const result = await withStepUp(() =>
            purgeArchiveItems({ archive_item_ids: [item.id], purge_reason: reason }),
          );
          if (result.purged_item_ids.includes(item.id)) {
            pushAppToast({ title: t("detail.purged", { name }), variant: "success" });
            await onChanged();
            onBack();
          } else {
            pushAppToast({ title: t("detail.purgeRefused"), variant: "destructive" });
            await load();
          }
        }}
      >
        <ul className="space-y-1 text-sm" aria-label={t("detail.eligibility")}>
          {purgeCheck.checks.map((c) => (
            <li key={c.key} className="flex items-start gap-2">
              <StatusChip
                label={c.ok ? t("detail.checkOk") : t("detail.checkBlocked")}
                tone={c.ok ? "success" : "danger"}
              />
              <span className="text-text-secondary">{c.label}</span>
            </li>
          ))}
        </ul>
      </ArchiveReasonDialog>
    </div>
  );
}

/** Mirrors the backend purge rules so the user sees why a purge would be refused. */
export function evaluatePurgeEligibility(
  detail: ArchiveItemDetail,
  t: TFunction<"archive">,
  formatDate: (v: string | Date) => string,
): { eligible: boolean; checks: { key: string; ok: boolean; label: string }[] } {
  const checks: { key: string; ok: boolean; label: string }[] = [];
  checks.push({
    key: "hold",
    ok: !detail.item.legal_hold,
    label: detail.item.legal_hold ? t("purgeCheck.holdOn") : t("purgeCheck.holdOff"),
  });
  const policy = detail.retention_policy;
  if (!policy) {
    checks.push({ key: "policy", ok: false, label: t("purgeCheck.noPolicy") });
    return { eligible: false, checks };
  }
  checks.push({
    key: "allow",
    ok: policy.allow_purge && policy.purge_mode !== "never",
    label:
      policy.allow_purge && policy.purge_mode !== "never"
        ? t("purgeCheck.policyAllows")
        : t("purgeCheck.policyForbids"),
  });
  const archivedAt = new Date(detail.item.archived_at);
  if (Number.isNaN(archivedAt.getTime())) {
    checks.push({ key: "period", ok: false, label: t("purgeCheck.invalidDate") });
  } else {
    const threshold = new Date(archivedAt);
    threshold.setUTCFullYear(threshold.getUTCFullYear() + policy.retention_years);
    const elapsed = Date.now() >= threshold.getTime();
    checks.push({
      key: "period",
      ok: elapsed,
      label: elapsed
        ? t("purgeCheck.periodElapsed", { years: policy.retention_years })
        : t("purgeCheck.periodRunning", { date: formatDate(threshold) }),
    });
  }
  return { eligible: checks.every((c) => c.ok), checks };
}

import {
  AlertTriangle,
  Archive,
  CalendarDays,
  ClipboardList,
  Database,
  FileJson,
  FolderTree,
  ShieldAlert,
  Wrench,
} from "lucide-react";
import { type ElementType, useCallback, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
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
import { toErrorMessage } from "@/utils/errors";

interface ArchiveExplorerProps {
  className?: string;
}

const LIST_PAGE_SIZE = 25;

export function ArchiveExplorer({ className }: ArchiveExplorerProps) {
  const { can, isLoading: permissionsLoading } = usePermissions();

  const [items, setItems] = useState<ArchiveItemSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [page, setPage] = useState(0);

  const [sourceModule, setSourceModule] = useState<string | undefined>(undefined);
  const [searchText, setSearchText] = useState("");
  const [legalHoldOnly, setLegalHoldOnly] = useState(false);
  const [selectedClasses, setSelectedClasses] = useState<Set<string>>(new Set());
  const [dateFrom, setDateFrom] = useState<string>("");
  const [dateTo, setDateTo] = useState<string>("");

  const [selectedDetail, setSelectedDetail] = useState<ArchiveItemDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const loadItems = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const filter: {
        source_module?: string;
        legal_hold?: boolean;
        search_text?: string;
        date_from?: string;
        date_to?: string;
        limit: number;
        offset: number;
      } = {
        limit: 500,
        offset: 0,
      };
      if (sourceModule) filter.source_module = sourceModule;
      if (legalHoldOnly) filter.legal_hold = true;
      if (searchText.trim()) filter.search_text = searchText.trim();
      if (dateFrom) filter.date_from = dateFrom;
      if (dateTo) filter.date_to = `${dateTo}T23:59:59Z`;

      const data = await listArchiveItems(filter);
      setItems(data);
      setPage(0);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [dateFrom, dateTo, legalHoldOnly, searchText, sourceModule]);

  useEffect(() => {
    void loadItems();
  }, [loadItems]);

  useEffect(() => {
    if (!selectedId) {
      setSelectedDetail(null);
      setDetailError(null);
      return;
    }

    let cancelled = false;
    setDetailLoading(true);
    setDetailError(null);
    void getArchiveItem(selectedId)
      .then((detail) => {
        if (!cancelled) {
          setSelectedDetail(detail);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setDetailError(toErrorMessage(err));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDetailLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const moduleClassYearTree = useMemo(() => {
    const tree = new Map<string, Map<string, Map<string, number>>>();
    for (const item of items) {
      const moduleMap = tree.get(item.source_module) ?? new Map<string, Map<string, number>>();
      const classMap = moduleMap.get(item.archive_class) ?? new Map<string, number>();
      const year = new Date(item.archived_at).getUTCFullYear();
      const yearKey = Number.isFinite(year) ? String(year) : "unknown";
      classMap.set(yearKey, (classMap.get(yearKey) ?? 0) + 1);
      moduleMap.set(item.archive_class, classMap);
      tree.set(item.source_module, moduleMap);
    }
    return tree;
  }, [items]);

  const availableClasses = useMemo(() => {
    const classes = new Set<string>();
    for (const item of items) {
      classes.add(item.archive_class);
    }
    return Array.from(classes).sort();
  }, [items]);

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
    const total = filteredItems.length;
    const legalHoldCount = filteredItems.filter((i) => i.legal_hold).length;
    const purgeEligibleNoHoldCount = filteredItems.filter((i) => !i.legal_hold).length;
    return { total, legalHoldCount, purgeEligibleNoHoldCount };
  }, [filteredItems]);

  const toggleClassFilter = useCallback((archiveClass: string) => {
    setSelectedClasses((prev) => {
      const next = new Set(prev);
      if (next.has(archiveClass)) {
        next.delete(archiveClass);
      } else {
        next.add(archiveClass);
      }
      return next;
    });
    setPage(0);
  }, []);

  const toggleSelectedRow = useCallback((archiveItemId: number, checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (checked) {
        next.add(archiveItemId);
      } else {
        next.delete(archiveItemId);
      }
      return next;
    });
  }, []);

  const selectedRows = useMemo(
    () => filteredItems.filter((item) => selectedIds.has(item.id)),
    [filteredItems, selectedIds],
  );

  const clearBulkSelection = useCallback(() => setSelectedIds(new Set()), []);

  const runBulkExport = useCallback(async () => {
    if (selectedRows.length === 0) return;
    setError(null);
    try {
      const payload = await exportArchiveItems({
        archive_item_ids: selectedRows.map((r) => r.id),
        export_reason: "Bulk export from archive explorer",
      });
      downloadAsJson(`archive-export-${new Date().toISOString()}.json`, payload);
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [selectedRows]);

  const runBulkLegalHold = useCallback(async () => {
    if (selectedRows.length === 0) return;
    const reason = window.prompt("Reason for legal hold (required):");
    if (!reason?.trim()) return;
    setError(null);
    try {
      for (const row of selectedRows) {
        await setLegalHold({
          archive_item_id: row.id,
          enable: true,
          reason_note: reason.trim(),
        });
      }
      clearBulkSelection();
      await loadItems();
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [clearBulkSelection, loadItems, selectedRows]);

  const runBulkPurge = useCallback(async () => {
    if (selectedRows.length === 0) return;
    const reason = window.prompt("Reason for purge (required):");
    if (!reason?.trim()) return;
    setError(null);
    try {
      await purgeArchiveItems({
        archive_item_ids: selectedRows.map((row) => row.id),
        purge_reason: reason.trim(),
      });
      clearBulkSelection();
      await loadItems();
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [clearBulkSelection, loadItems, selectedRows]);

  return (
    <div className={className}>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[300px_minmax(0,1fr)]">
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">Archive Explorer</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="rounded-md border p-3">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium">
                <FolderTree className="h-4 w-4" />
                Folder tree
              </div>
              <div
                className="max-h-60 overflow-auto pr-1 text-sm"
                data-testid="archive-folder-tree"
              >
                {Array.from(moduleClassYearTree.entries())
                  .sort(([a], [b]) => a.localeCompare(b))
                  .map(([module, classMap]) => (
                    <details key={module} className="mb-1">
                      <summary
                        className="cursor-pointer rounded px-1 py-0.5 hover:bg-muted"
                        onClick={(e) => {
                          e.preventDefault();
                          setSourceModule((prev) => (prev === module ? undefined : module));
                        }}
                      >
                        <span className={sourceModule === module ? "font-semibold" : undefined}>
                          {module}
                        </span>
                      </summary>
                      <div className="ml-3 mt-1 space-y-1 border-l pl-2">
                        {Array.from(classMap.entries())
                          .sort(([a], [b]) => a.localeCompare(b))
                          .map(([archiveClass, yearMap]) => (
                            <details key={`${module}-${archiveClass}`}>
                              <summary className="cursor-pointer rounded px-1 py-0.5 hover:bg-muted">
                                <button
                                  type="button"
                                  className="text-left"
                                  onClick={(event) => {
                                    event.preventDefault();
                                    setSelectedClasses(new Set([archiveClass]));
                                    setPage(0);
                                  }}
                                >
                                  {archiveClass}
                                </button>
                              </summary>
                              <div className="ml-3 mt-1 space-y-1 border-l pl-2">
                                {Array.from(yearMap.entries())
                                  .sort(([a], [b]) => b.localeCompare(a))
                                  .map(([year, count]) => (
                                    <button
                                      key={`${module}-${archiveClass}-${year}`}
                                      type="button"
                                      className="flex w-full items-center justify-between rounded px-1 py-0.5 text-left hover:bg-muted"
                                      onClick={() => {
                                        setSelectedClasses(new Set([archiveClass]));
                                        setDateFrom(`${year}-01-01`);
                                        setDateTo(`${year}-12-31`);
                                        setPage(0);
                                      }}
                                    >
                                      <span>{year}</span>
                                      <Badge variant="outline" className="text-[10px]">
                                        {count}
                                      </Badge>
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
                placeholder="Search archived text..."
              />
              <div className="flex items-center gap-2 text-sm">
                <Checkbox
                  id="archive-legal-hold-only"
                  checked={legalHoldOnly}
                  onCheckedChange={(checked) => setLegalHoldOnly(checked)}
                />
                <label htmlFor="archive-legal-hold-only" className="cursor-pointer">
                  Legal Hold only
                </label>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">Archive class filters</p>
                <div className="flex flex-wrap gap-1">
                  {availableClasses.map((archiveClass) => {
                    const active = selectedClasses.has(archiveClass);
                    return (
                      <button
                        key={archiveClass}
                        type="button"
                        className={`rounded-full border px-2 py-0.5 text-xs ${active ? "border-primary bg-primary/10 text-primary" : "border-border"}`}
                        onClick={() => toggleClassFilter(archiveClass)}
                      >
                        {archiveClass}
                      </button>
                    );
                  })}
                </div>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <Input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} />
                <Input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
              </div>
              <div className="flex gap-2">
                <Button size="sm" onClick={() => void loadItems()} disabled={loading}>
                  Apply
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setSourceModule(undefined);
                    setSearchText("");
                    setLegalHoldOnly(false);
                    setSelectedClasses(new Set());
                    setDateFrom("");
                    setDateTo("");
                    setPage(0);
                  }}
                >
                  Reset
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">Details</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {error && <div className="text-sm text-destructive">{error}</div>}
            {loading && <div className="text-sm text-muted-foreground">Loading archive items...</div>}

            {selectedRows.length > 0 && !permissionsLoading && (
              <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/40 p-2">
                <span className="text-sm text-muted-foreground">{selectedRows.length} selected</span>
                {can("arc.export") && (
                  <Button size="sm" variant="outline" onClick={() => void runBulkExport()}>
                    Export Selected
                  </Button>
                )}
                {can("arc.purge") && (
                  <Button size="sm" variant="outline" onClick={() => void runBulkLegalHold()}>
                    Legal Hold (all)
                  </Button>
                )}
                {can("arc.purge") && (
                  <Button size="sm" variant="destructive" onClick={() => void runBulkPurge()}>
                    Purge (eligible only)
                  </Button>
                )}
                <Button size="sm" variant="ghost" onClick={clearBulkSelection}>
                  Clear
                </Button>
              </div>
            )}

            {!selectedId ? (
              <>
                <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                  <StatCard label="Total archived" value={stats.total} icon={Archive} />
                  <StatCard label="Legal hold" value={stats.legalHoldCount} icon={ShieldAlert} />
                  <StatCard
                    label="Purge-eligible (no hold)"
                    value={stats.purgeEligibleNoHoldCount}
                    icon={AlertTriangle}
                  />
                </div>

                <div className="rounded-md border">
                  <div className="grid grid-cols-[30px_1fr_auto_auto_auto] gap-2 border-b bg-muted/40 px-3 py-2 text-xs font-medium text-muted-foreground">
                    <span />
                    <span>Record</span>
                    <span>Class</span>
                    <span>Status</span>
                    <span>Archived</span>
                  </div>
                  <div className="max-h-[540px] overflow-auto">
                    {visibleItems.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        className="grid w-full grid-cols-[30px_1fr_auto_auto_auto] gap-2 border-b px-3 py-2 text-left text-sm hover:bg-muted/30"
                        onClick={() => setSelectedId(item.id)}
                      >
                        <Checkbox
                          checked={selectedIds.has(item.id)}
                          onCheckedChange={(checked) => toggleSelectedRow(item.id, checked)}
                          onClick={(e) => e.stopPropagation()}
                        />
                        <div className="min-w-0">
                          <p className="truncate font-medium">{item.source_record_id}</p>
                          <p className="truncate text-xs text-muted-foreground">{item.source_module}</p>
                        </div>
                        <Badge variant="outline" className="h-fit">
                          {item.archive_class}
                        </Badge>
                        {item.legal_hold ? (
                          <Badge className="h-fit bg-orange-500 text-white">Legal hold</Badge>
                        ) : (
                          <Badge variant="secondary" className="h-fit">
                            normal
                          </Badge>
                        )}
                        <span className="whitespace-nowrap text-xs text-muted-foreground">
                          {formatShortDate(item.archived_at)}
                        </span>
                      </button>
                    ))}
                    {visibleItems.length === 0 && !loading && (
                      <div className="p-4 text-sm text-muted-foreground">No archived items found.</div>
                    )}
                  </div>
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-xs text-muted-foreground">
                    Showing {Math.min(filteredItems.length, page * LIST_PAGE_SIZE + visibleItems.length)} of{" "}
                    {filteredItems.length}
                  </span>
                  <div className="flex gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={page === 0}
                      onClick={() => setPage((prev) => Math.max(0, prev - 1))}
                    >
                      Previous
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={!hasMore}
                      onClick={() => setPage((prev) => prev + 1)}
                    >
                      Next
                    </Button>
                  </div>
                </div>
              </>
            ) : (
              <ArchiveItemDetailView
                detail={selectedDetail}
                loading={detailLoading}
                error={detailError}
                onBack={() => setSelectedId(null)}
                onRefresh={async () => {
                  await loadItems();
                  if (selectedId) {
                    setSelectedDetail(await getArchiveItem(selectedId));
                  }
                }}
                canExport={can("arc.export")}
                canRestore={can("arc.restore")}
                canPurge={can("arc.purge")}
              />
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

interface ArchiveItemDetailViewProps {
  detail: ArchiveItemDetail | null;
  loading: boolean;
  error: string | null;
  canExport: boolean;
  canRestore: boolean;
  canPurge: boolean;
  onBack: () => void;
  onRefresh: () => Promise<void>;
}

function ArchiveItemDetailView({
  detail,
  loading,
  error,
  canExport,
  canRestore,
  canPurge,
  onBack,
  onRefresh,
}: ArchiveItemDetailViewProps) {
  const [tab, setTab] = useState<"record" | "history">("record");

  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false);
  const [restoreReason, setRestoreReason] = useState("");
  const [legalHoldDialogOpen, setLegalHoldDialogOpen] = useState(false);
  const [legalHoldReason, setLegalHoldReason] = useState("");
  const [purgeDialogOpen, setPurgeDialogOpen] = useState(false);
  const [purgeReason, setPurgeReason] = useState("");
  const [purgeStep, setPurgeStep] = useState<1 | 2>(1);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  if (loading) {
    return <div className="text-sm text-muted-foreground">Loading detail...</div>;
  }
  if (error) {
    return <div className="text-sm text-destructive">{error}</div>;
  }
  if (!detail) {
    return <div className="text-sm text-muted-foreground">Select an archive item to inspect.</div>;
  }

  const purgeCheck = evaluatePurgeEligibility(detail);
  const restoreBlocked = detail.item.restore_policy === "not_allowed";
  const moduleIcon = moduleIconFor(detail.item.source_module);

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Button size="sm" variant="outline" onClick={onBack}>
          Back to list
        </Button>
      </div>

      <div className="rounded-md border p-3">
        <div className="flex flex-wrap items-center gap-2">
          {moduleIcon}
          <span className="font-medium">{detail.item.source_record_id}</span>
          <Badge variant="outline">{detail.item.archive_class}</Badge>
          {detail.item.legal_hold && <Badge className="bg-orange-500 text-white">Legal hold</Badge>}
          <Badge variant={detail.checksum_valid ? "outline" : "destructive"}>
            {detail.checksum_valid ? "Checksum verified" : "Checksum mismatch"}
          </Badge>
        </div>
        <p className="mt-2 text-xs text-muted-foreground">
          Module: {detail.item.source_module} • Archived: {formatShortDate(detail.item.archived_at)}
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        {canExport && (
          <Button
            size="sm"
            variant="outline"
            disabled={actionBusy}
            onClick={() =>
              void (async () => {
                setActionError(null);
                setActionBusy(true);
                try {
                  const payload = await exportArchiveItems({
                    archive_item_ids: [detail.item.id],
                    export_reason: "Single item export",
                  });
                  downloadAsJson(`archive-${detail.item.id}.json`, payload.items[0]?.payload_json ?? payload);
                } catch (err) {
                  setActionError(toErrorMessage(err));
                } finally {
                  setActionBusy(false);
                }
              })()
            }
          >
            Export
          </Button>
        )}

        {canRestore && (
          <Button
            size="sm"
            variant="outline"
            disabled={restoreBlocked}
            title={restoreBlocked ? "Restore blocked: restore_policy is not_allowed" : undefined}
            onClick={() => setRestoreDialogOpen(true)}
          >
            Restore
          </Button>
        )}

        {canPurge && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => setLegalHoldDialogOpen(true)}
          >
            {detail.item.legal_hold ? "Legal Hold Off" : "Legal Hold On"}
          </Button>
        )}

        {canPurge && (
          <Button
            size="sm"
            variant="destructive"
            disabled={!purgeCheck.eligible}
            title={!purgeCheck.eligible ? purgeCheck.reasons.join(" | ") : undefined}
            onClick={() => {
              setPurgeStep(1);
              setPurgeDialogOpen(true);
            }}
          >
            Purge
          </Button>
        )}
      </div>

      {actionError && <div className="text-sm text-destructive">{actionError}</div>}

      <Tabs value={tab} onValueChange={(value) => setTab(value as "record" | "history")}>
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="record">Record Data</TabsTrigger>
          <TabsTrigger value="history">History</TabsTrigger>
        </TabsList>
      </Tabs>

      {tab === "record" ? (
        <div className="space-y-2 rounded-md border p-3">
          <div className="grid grid-cols-1 gap-2 text-sm md:grid-cols-2">
            <div>
              <span className="text-muted-foreground">Restore policy:</span> {detail.item.restore_policy}
            </div>
            <div>
              <span className="text-muted-foreground">Retention policy:</span>{" "}
              {detail.retention_policy ? `${detail.retention_policy.module_code}/${detail.retention_policy.archive_class}` : "—"}
            </div>
          </div>

          <details open>
            <summary className="cursor-pointer text-sm font-medium">Decoded payload JSON</summary>
            <pre className="mt-2 max-h-[320px] overflow-auto rounded bg-muted p-2 text-xs">
              {JSON.stringify(detail.payload?.payload_json ?? null, null, 2)}
            </pre>
          </details>
        </div>
      ) : (
        <div className="space-y-2 rounded-md border p-3">
          {detail.actions.map((action) => (
            <div key={action.id} className="rounded border p-2 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline">{action.action}</Badge>
                <span className="text-xs text-muted-foreground">{formatShortDate(action.action_at)}</span>
                <Badge variant={action.result_status === "success" ? "outline" : "destructive"}>
                  {action.result_status}
                </Badge>
              </div>
              {action.reason_note && <p className="mt-1 text-xs text-muted-foreground">{action.reason_note}</p>}
            </div>
          ))}
          {detail.actions.length === 0 && (
            <div className="text-sm text-muted-foreground">No history events.</div>
          )}
        </div>
      )}

      <Dialog open={restoreDialogOpen} onOpenChange={setRestoreDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Confirm restore</DialogTitle>
            <DialogDescription>
              Restoration writes an archive action and defers record replay to module-specific handlers.
            </DialogDescription>
          </DialogHeader>
          <Textarea
            value={restoreReason}
            onChange={(e) => setRestoreReason(e.target.value)}
            placeholder="Reason for restore"
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setRestoreDialogOpen(false)}>
              Cancel
            </Button>
            <Button
              disabled={actionBusy || !restoreReason.trim()}
              onClick={() =>
                void (async () => {
                  setActionError(null);
                  setActionBusy(true);
                  try {
                    await restoreArchiveItem({
                      archive_item_id: detail.item.id,
                      reason_note: restoreReason.trim(),
                    });
                    setRestoreDialogOpen(false);
                    setRestoreReason("");
                    await onRefresh();
                  } catch (err) {
                    setActionError(toErrorMessage(err));
                  } finally {
                    setActionBusy(false);
                  }
                })()
              }
            >
              Confirm restore
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={legalHoldDialogOpen} onOpenChange={setLegalHoldDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{detail.item.legal_hold ? "Disable legal hold" : "Enable legal hold"}</DialogTitle>
            <DialogDescription>Reason is required and will be recorded in archive actions.</DialogDescription>
          </DialogHeader>
          <Textarea
            value={legalHoldReason}
            onChange={(e) => setLegalHoldReason(e.target.value)}
            placeholder="Reason note"
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setLegalHoldDialogOpen(false)}>
              Cancel
            </Button>
            <Button
              disabled={actionBusy || !legalHoldReason.trim()}
              onClick={() =>
                void (async () => {
                  setActionError(null);
                  setActionBusy(true);
                  try {
                    await setLegalHold({
                      archive_item_id: detail.item.id,
                      enable: !detail.item.legal_hold,
                      reason_note: legalHoldReason.trim(),
                    });
                    setLegalHoldDialogOpen(false);
                    setLegalHoldReason("");
                    await onRefresh();
                  } catch (err) {
                    setActionError(toErrorMessage(err));
                  } finally {
                    setActionBusy(false);
                  }
                })()
              }
            >
              Confirm
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={purgeDialogOpen} onOpenChange={setPurgeDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Purge archived item</DialogTitle>
            <DialogDescription>
              Step {purgeStep} of 2 — this action removes archive payload and item rows.
            </DialogDescription>
          </DialogHeader>

          {purgeStep === 1 ? (
            <div className="space-y-2">
              <p className="text-sm font-medium">Eligibility summary</p>
              <ul className="list-disc space-y-1 pl-4 text-sm text-muted-foreground">
                {purgeCheck.reasons.map((reason) => (
                  <li key={reason}>{reason}</li>
                ))}
              </ul>
              <DialogFooter>
                <Button variant="outline" onClick={() => setPurgeDialogOpen(false)}>
                  Cancel
                </Button>
                <Button disabled={!purgeCheck.eligible} onClick={() => setPurgeStep(2)}>
                  Continue
                </Button>
              </DialogFooter>
            </div>
          ) : (
            <div className="space-y-2">
              <Textarea
                value={purgeReason}
                onChange={(e) => setPurgeReason(e.target.value)}
                placeholder="Purge reason"
              />
              <DialogFooter>
                <Button variant="outline" onClick={() => setPurgeStep(1)}>
                  Back
                </Button>
                <Button
                  variant="destructive"
                  disabled={actionBusy || !purgeReason.trim()}
                  onClick={() =>
                    void (async () => {
                      setActionError(null);
                      setActionBusy(true);
                      try {
                        await purgeArchiveItems({
                          archive_item_ids: [detail.item.id],
                          purge_reason: purgeReason.trim(),
                        });
                        setPurgeDialogOpen(false);
                        setPurgeReason("");
                        await onRefresh();
                        onBack();
                      } catch (err) {
                        setActionError(toErrorMessage(err));
                      } finally {
                        setActionBusy(false);
                      }
                    })()
                  }
                >
                  Confirm purge
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function evaluatePurgeEligibility(detail: ArchiveItemDetail): { eligible: boolean; reasons: string[] } {
  const reasons: string[] = [];
  if (detail.item.legal_hold) {
    reasons.push("Blocked: legal hold is enabled.");
  } else {
    reasons.push("Legal hold check: passed.");
  }

  const policy = detail.retention_policy;
  if (!policy) {
    reasons.push("Blocked: no retention policy linked to this item.");
    return { eligible: false, reasons };
  }

  if (!policy.allow_purge) {
    reasons.push("Blocked: retention policy does not allow purge.");
  } else {
    reasons.push("Policy check: allow_purge = true.");
  }

  if (policy.purge_mode === "never") {
    reasons.push("Blocked: purge_mode is never.");
  } else {
    reasons.push(`Policy check: purge_mode is ${policy.purge_mode}.`);
  }

  const archivedAt = new Date(detail.item.archived_at);
  const threshold = new Date(archivedAt);
  threshold.setUTCFullYear(threshold.getUTCFullYear() + policy.retention_years);
  if (Number.isNaN(archivedAt.getTime())) {
    reasons.push("Blocked: archived_at is invalid.");
  } else if (Date.now() < threshold.getTime()) {
    reasons.push(
      `Blocked: retention period not elapsed (eligible after ${threshold.toISOString().slice(0, 10)}).`,
    );
  } else {
    reasons.push("Retention period check: elapsed.");
  }

  const blocked = reasons.some((r) => r.startsWith("Blocked:"));
  return { eligible: !blocked, reasons };
}

function moduleIconFor(moduleCode: string) {
  const code = moduleCode.toLowerCase();
  if (code.includes("wo")) return <Wrench className="h-4 w-4 text-blue-500" />;
  if (code.includes("di")) return <ClipboardList className="h-4 w-4 text-violet-500" />;
  if (code.includes("rbac")) return <ShieldAlert className="h-4 w-4 text-amber-500" />;
  if (code.includes("report")) return <FileJson className="h-4 w-4 text-green-600" />;
  if (code.includes("config")) return <CalendarDays className="h-4 w-4 text-cyan-600" />;
  return <Database className="h-4 w-4 text-muted-foreground" />;
}

function StatCard({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: number;
  icon: ElementType;
}) {
  return (
    <div className="rounded-md border p-3">
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Icon className="h-4 w-4" />
        {label}
      </div>
      <div className="mt-1 text-xl font-semibold">{value}</div>
    </div>
  );
}

function downloadAsJson(fileName: string, payload: unknown) {
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
}

function formatShortDate(value: string): string {
  const dt = new Date(value);
  if (Number.isNaN(dt.getTime())) return value;
  return dt.toLocaleString();
}

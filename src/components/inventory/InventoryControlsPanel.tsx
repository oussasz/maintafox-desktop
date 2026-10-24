import type { ColumnDef } from "@tanstack/react-table";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { useLookupOptions } from "@/components/inventory/use-lookup-options";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { COUNT_SESSION_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { useSession } from "@/hooks/use-session";
import {
  type InventoryArticleRow,
  type InventoryCountLineRow,
  approveInventoryCountLine,
  createInventoryCountSession,
  getInventoryCountTolerance,
  listInventoryArticles,
  listInventoryCountLines,
  listInventoryCountSessions,
  listInventoryLocations,
  listInventoryReconciliationFindings,
  listInventoryReconciliationRuns,
  listInventoryTransactions,
  listInventoryWarehouses,
  postInventoryCountSession,
  reverseInventoryCountSession,
  runInventoryReconciliation,
  transitionInventoryCountSession,
  upsertInventoryCountLine,
} from "@/services/inventory-service";
import type {
  InventoryCountSession,
  InventoryCountTolerance,
  InventoryReconciliationFinding,
  InventoryReconciliationRun,
  InventoryTransaction,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/**
 * Minimum length (characters, trimmed) of the mandatory reversal reason (audit trail): the rule of
 * a count's approval, since a reversal can reconcile a location below zero.
 */
const REVERSAL_REASON_MIN_LENGTH = 8;
/** Where the count and reconciliation tolerances are set (tenant settings, Inventory section). */
const INVENTORY_SETTINGS_PATH = "/settings?tab=general";
/** Count-session trace: most recent posted / reversed count movements shown. */
const COUNT_TRACE_LIMIT = 120;

export interface InventoryControlsPanelProps {
  /** Bumped by the page refresh button to reload every list. */
  reloadToken?: number;
}

export function InventoryControlsPanel({ reloadToken = 0 }: InventoryControlsPanelProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatCurrency, formatDateTime, formatNumber } = useFormatters();
  const { info } = useSession();
  const varianceReasons = useLookupOptions("inventory.variance_reason");
  const movementReasons = useLookupOptions("inventory.movement_type");
  const [warehouses, setWarehouses] = useState<Warehouse[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [articles, setArticles] = useState<InventoryArticleRow[]>([]);
  const [sessions, setSessions] = useState<InventoryCountSession[]>([]);
  /** The plant's four-eyes tolerance (Settings › Inventory); null until loaded. */
  const [tolerance, setTolerance] = useState<InventoryCountTolerance | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [lines, setLines] = useState<InventoryCountLineRow[]>([]);
  const [linesError, setLinesError] = useState<unknown>(null);
  const [runs, setRuns] = useState<InventoryReconciliationRun[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<number | null>(null);
  const [findings, setFindings] = useState<InventoryReconciliationFinding[]>([]);
  const [findingsError, setFindingsError] = useState<unknown>(null);
  const [traceRows, setTraceRows] = useState<InventoryTransaction[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);

  const [newWarehouseId, setNewWarehouseId] = useState(0);
  const [newLocationId, setNewLocationId] = useState(0);
  const [lineArticleId, setLineArticleId] = useState(0);
  const [lineLocationId, setLineLocationId] = useState(0);
  const [lineCountQty, setLineCountQty] = useState(0);
  const [lineVarianceReasonCode, setLineVarianceReasonCode] = useState("");
  const [approvalLine, setApprovalLine] = useState<InventoryCountLineRow | null>(null);
  const [approvalEvidence, setApprovalEvidence] = useState("");
  const [sessionReversalReason, setSessionReversalReason] = useState("");
  const [reverseConfirmOpen, setReverseConfirmOpen] = useState(false);

  const selectedSession = useMemo(
    () => sessions.find((session) => session.id === selectedSessionId) ?? null,
    [sessions, selectedSessionId],
  );

  const qty = useCallback(
    (value: number) => formatNumber(value, { maximumFractionDigits: 3 }),
    [formatNumber],
  );
  const sessionStatusLabel = useCallback(
    (status: string) => t(`controls.sessionStatuses.${status}`, { defaultValue: status }),
    [t],
  );
  const warehouseLabel = useCallback(
    (warehouseId: number) => {
      const w = warehouses.find((row) => row.id === warehouseId);
      return w ? `${w.code} — ${w.name}` : "—";
    },
    [warehouses],
  );

  const loadAll = useCallback(async () => {
    setLoading(true);
    try {
      const [warehouseRows, locationRows, articleRows, sessionRows, runRows, txRows, toleranceRow] =
        await Promise.all([
          listInventoryWarehouses(),
          listInventoryLocations(null),
          listInventoryArticles({ search: null }),
          listInventoryCountSessions(),
          listInventoryReconciliationRuns(),
          listInventoryTransactions({ source_type: "COUNT_SESSION", limit: COUNT_TRACE_LIMIT }),
          getInventoryCountTolerance(),
        ]);
      setTolerance(toleranceRow);
      setWarehouses(warehouseRows.filter((row) => row.is_active === 1));
      setLocations(locationRows.filter((row) => row.is_active === 1));
      setArticles(articleRows.filter((row) => row.is_active === 1));
      setSessions(sessionRows);
      setRuns(runRows);
      setTraceRows(txRows);
      setLoadError(null);
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadAll();
  }, [loadAll, reloadToken]);

  const refreshCountLines = useCallback(async () => {
    if (!selectedSessionId) {
      setLines([]);
      return;
    }
    try {
      setLines(await listInventoryCountLines(selectedSessionId));
      setLinesError(null);
    } catch (err) {
      setLinesError(err);
    }
  }, [selectedSessionId]);

  useEffect(() => {
    void refreshCountLines();
  }, [refreshCountLines, reloadToken]);

  useEffect(() => {
    if (!selectedRunId) {
      setFindings([]);
      return;
    }
    let cancelled = false;
    setFindingsError(null);
    listInventoryReconciliationFindings(selectedRunId)
      .then((rows) => {
        if (!cancelled) setFindings(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setFindingsError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedRunId]);

  /**
   * Runs a mutation, then reloads. Resolves to false when it failed (the global IPC toast
   * explains why), so dialogs can stay open with what the user typed.
   */
  const withSave = async (work: () => Promise<void>): Promise<boolean> => {
    setSaving(true);
    try {
      await work();
    } catch {
      return false;
    } finally {
      setSaving(false);
    }
    await loadAll();
    await refreshCountLines();
    return true;
  };

  const transitionSession = (nextStatus: string) => {
    if (!selectedSession) return;
    void withSave(async () => {
      await transitionInventoryCountSession({
        session_id: selectedSession.id,
        expected_row_version: selectedSession.row_version,
        next_status: nextStatus,
        // The actor is recorded by the backend from the signed-in session.
        actor_id: info?.user_id ?? null,
      });
    });
  };

  const approveSelectedLine = async () => {
    if (!approvalLine || !approvalEvidence.trim()) return;
    const ok = await withSave(async () => {
      await approveInventoryCountLine({
        line_id: approvalLine.id,
        expected_row_version: approvalLine.row_version,
        // The backend records the signed-in user as reviewer.
        reviewer_id: info?.user_id ?? 0,
        reviewer_evidence: approvalEvidence.trim(),
      });
    });
    if (!ok) return;
    setApprovalLine(null);
    setApprovalEvidence("");
  };

  /** The four-eyes tolerance in words: the thresholds set, or "not set: every variance". */
  const toleranceText = (() => {
    if (!tolerance) return "";
    const parts: string[] = [];
    if (tolerance.value_threshold != null) {
      parts.push(
        t("controls.fourEyes.value", { value: formatCurrency(tolerance.value_threshold) }),
      );
    }
    if (tolerance.percent_threshold != null) {
      parts.push(
        t("controls.fourEyes.percent", {
          value: formatNumber(tolerance.percent_threshold, { maximumFractionDigits: 6 }),
        }),
      );
    }
    return parts.length > 0
      ? t("controls.fourEyes.set", { thresholds: parts.join(t("controls.fourEyes.or")) })
      : t("controls.fourEyes.notSet");
  })();

  const pendingApprovals = lines.filter(
    (line) => line.approval_required === 1 && line.approved_by_id === null,
  ).length;

  const lineColumns: ColumnDef<InventoryCountLineRow>[] = [
    {
      accessorKey: "article_code",
      header: t("controls.lines.columns.article"),
      cell: ({ row }) => (
        <span className="text-xs">
          <span className="font-mono">{row.original.article_code}</span> —{" "}
          {row.original.article_name}
        </span>
      ),
    },
    { accessorKey: "location_code", header: t("controls.lines.columns.location") },
    {
      accessorKey: "system_qty",
      header: t("controls.lines.columns.systemQty"),
      cell: ({ row }) => <span className="tabular-nums">{qty(row.original.system_qty)}</span>,
    },
    {
      accessorKey: "counted_qty",
      header: t("controls.lines.columns.countedQty"),
      cell: ({ row }) => <span className="tabular-nums">{qty(row.original.counted_qty)}</span>,
    },
    {
      accessorKey: "variance_qty",
      header: t("controls.lines.columns.variance"),
      cell: ({ row }) => (
        <span className="tabular-nums">
          {row.original.variance_qty > 0 ? "+" : ""}
          {qty(row.original.variance_qty)}
          {row.original.variance_reason_code ? (
            <span className="ml-1 text-xs text-text-muted">
              ({varianceReasons.labelOf(row.original.variance_reason_code)})
            </span>
          ) : null}
        </span>
      ),
    },
    {
      accessorKey: "is_critical",
      header: t("controls.lines.columns.critical"),
      cell: ({ row }) =>
        row.original.is_critical === 1 ? (
          <span title={t("controls.lines.criticalMeaning")}>
            <StatusChip label={t("controls.lines.critical")} tone="danger" />
          </span>
        ) : (
          <StatusChip label={t("controls.lines.normal")} tone="neutral" />
        ),
    },
    {
      id: "counted_by",
      header: t("controls.lines.columns.countedBy"),
      cell: ({ row }) => (
        <span className="text-xs">
          {row.original.counted_by_name ?? t("controls.lines.counterNotRecorded")}
        </span>
      ),
    },
    {
      id: "review",
      header: t("controls.lines.columns.review"),
      cell: ({ row }) => {
        const line = row.original;
        if (line.approved_by_id !== null) {
          return (
            <span
              className="flex flex-wrap items-center gap-1 text-xs"
              title={line.approval_note ?? undefined}
            >
              {t("controls.lines.approvedBy", {
                name: line.approved_by_name ?? t("controls.lines.unknownReviewer"),
                date: line.approved_at ? formatDateTime(line.approved_at) : "—",
              })}
              {line.self_approved === 1 ? (
                <span title={t("controls.lines.selfApprovedMeaning")}>
                  <StatusChip label={t("controls.lines.selfApproved")} tone="warning" />
                </span>
              ) : null}
            </span>
          );
        }
        if (line.approval_required !== 1) {
          return (
            <span className="text-xs text-text-muted">{t("controls.lines.noApprovalNeeded")}</span>
          );
        }
        return (
          <PermissionGate
            permission={P.INV_MANAGE}
            fallback={
              <span className="text-xs text-text-muted">{t("controls.lines.pendingApproval")}</span>
            }
          >
            <Button
              variant="outline"
              className="h-9"
              disabled={saving}
              onClick={() => {
                setApprovalLine(line);
                setApprovalEvidence("");
              }}
            >
              {t("controls.lines.approve")}
            </Button>
          </PermissionGate>
        );
      },
    },
  ];

  const findingColumns: ColumnDef<InventoryReconciliationFinding>[] = [
    { accessorKey: "article_code", header: t("controls.findings.columns.article") },
    { accessorKey: "warehouse_code", header: t("controls.findings.columns.warehouse") },
    { accessorKey: "location_code", header: t("controls.findings.columns.location") },
    {
      accessorKey: "balance_on_hand",
      header: t("controls.findings.columns.balance"),
      cell: ({ row }) => <span className="tabular-nums">{qty(row.original.balance_on_hand)}</span>,
    },
    {
      accessorKey: "ledger_expected_on_hand",
      header: t("controls.findings.columns.ledger"),
      cell: ({ row }) => (
        <span className="tabular-nums">{qty(row.original.ledger_expected_on_hand)}</span>
      ),
    },
    {
      accessorKey: "drift_qty",
      header: t("controls.findings.columns.drift"),
      cell: ({ row }) => <span className="tabular-nums">{qty(row.original.drift_qty)}</span>,
    },
    {
      accessorKey: "is_break",
      header: t("controls.findings.columns.severity"),
      cell: ({ row }) =>
        row.original.is_break === 1 ? (
          <StatusChip label={t("controls.findings.break")} tone="danger" />
        ) : (
          <StatusChip label={t("controls.findings.info")} tone="info" />
        ),
    },
  ];

  const traceColumns: ColumnDef<InventoryTransaction>[] = [
    {
      accessorKey: "performed_at",
      header: t("controls.trace.columns.when"),
      cell: ({ row }) => (
        <span className="text-xs">{formatDateTime(row.original.performed_at)}</span>
      ),
    },
    {
      accessorKey: "movement_type",
      header: t("controls.trace.columns.movement"),
      cell: ({ row }) =>
        t(`detail.activity.types.movement.${row.original.movement_type}`, {
          defaultValue: t("detail.activity.types.other"),
        }),
    },
    { accessorKey: "article_code", header: t("controls.trace.columns.article") },
    { accessorKey: "location_code", header: t("controls.trace.columns.location") },
    {
      accessorKey: "quantity",
      header: t("controls.trace.columns.quantity"),
      cell: ({ row }) => <span className="tabular-nums">{qty(row.original.quantity)}</span>,
    },
    { accessorKey: "source_ref", header: t("controls.trace.columns.sourceRef") },
    {
      accessorKey: "reason_code",
      header: t("controls.trace.columns.reason"),
      cell: ({ row }) =>
        row.original.reason_code
          ? varianceReasons.options.some((o) => o.code === row.original.reason_code)
            ? varianceReasons.labelOf(row.original.reason_code)
            : movementReasons.labelOf(row.original.reason_code)
          : "—",
    },
    { accessorKey: "notes", header: t("controls.trace.columns.notes") },
  ];

  if (loadError && sessions.length === 0 && warehouses.length === 0) {
    return (
      <ErrorState
        error={loadError}
        title={t("controls.loadError")}
        onRetry={() => void loadAll()}
        retrying={loading}
      />
    );
  }

  const lineBlocker =
    lineArticleId <= 0
      ? t("controls.lines.blockers.article")
      : lineLocationId <= 0
        ? t("controls.lines.blockers.location")
        : null;

  return (
    <div className="space-y-4">
      {loadError ? (
        <ErrorState
          error={loadError}
          title={t("controls.loadError")}
          onRetry={() => void loadAll()}
          retrying={loading}
        />
      ) : null}

      <PermissionGate permission={P.INV_COUNT}>
        <section className="rounded-md border p-4">
          <h3 className="mb-1 text-sm font-semibold">{t("controls.sessions.title")}</h3>
          <p className="mb-1 text-xs text-text-muted">{t("controls.sessions.hint")}</p>
          <p className="mb-1 text-xs text-text-muted">
            {t("controls.sessions.criticalFromSettings")}{" "}
            <Link to={INVENTORY_SETTINGS_PATH} className="underline underline-offset-2">
              {t("controls.setInSettings")}
            </Link>
          </p>
          <p className="mb-3 text-xs text-text-secondary" data-testid="inv-cc-four-eyes-tolerance">
            {toleranceText}
          </p>
          <div className="grid gap-2 md:grid-cols-3">
            <div className="space-y-1">
              <Label htmlFor="inv-cc-warehouse" className="text-xs">
                {t("controls.sessions.warehouse")}
              </Label>
              <Select
                value={String(newWarehouseId)}
                onValueChange={(v) => {
                  setNewWarehouseId(Number(v));
                  setNewLocationId(0);
                }}
              >
                <SelectTrigger id="inv-cc-warehouse">
                  <SelectValue placeholder={t("controls.sessions.selectWarehouse")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("controls.sessions.selectWarehouse")}</SelectItem>
                  {warehouses.map((w) => (
                    <SelectItem key={w.id} value={String(w.id)}>
                      {w.code} - {w.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-cc-location" className="text-xs">
                {t("controls.sessions.scopeLocation")}
              </Label>
              <Select
                value={String(newLocationId)}
                onValueChange={(v) => setNewLocationId(Number(v))}
              >
                <SelectTrigger id="inv-cc-location">
                  <SelectValue placeholder={t("controls.sessions.allLocations")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("controls.sessions.allLocations")}</SelectItem>
                  {locations
                    .filter((l) => newWarehouseId <= 0 || l.warehouse_id === newWarehouseId)
                    .map((l) => (
                      <SelectItem key={l.id} value={String(l.id)}>
                        {l.warehouse_code}/{l.code}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-end">
              <Button
                className="h-11"
                disabled={saving || newWarehouseId <= 0}
                title={newWarehouseId <= 0 ? t("controls.sessions.selectWarehouse") : undefined}
                onClick={() =>
                  void withSave(async () => {
                    const session = await createInventoryCountSession({
                      warehouse_id: newWarehouseId,
                      location_id: newLocationId > 0 ? newLocationId : null,
                      actor_id: info?.user_id ?? null,
                    });
                    setSelectedSessionId(session.id);
                  })
                }
              >
                {t("controls.sessions.create")}
              </Button>
            </div>
          </div>
          <div className="mt-3 flex flex-wrap items-end gap-2">
            <div className="space-y-1">
              <Label htmlFor="inv-cc-session" className="text-xs">
                {t("controls.sessions.session")}
              </Label>
              <Select
                value={String(selectedSessionId ?? 0)}
                onValueChange={(v) => setSelectedSessionId(Number(v) || null)}
              >
                <SelectTrigger id="inv-cc-session" className="w-80">
                  <SelectValue placeholder={t("controls.sessions.selectSession")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("controls.sessions.selectSession")}</SelectItem>
                  {sessions.map((session) => (
                    <SelectItem key={session.id} value={String(session.id)}>
                      {session.session_code} ({sessionStatusLabel(session.status)})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {selectedSession ? (
              <>
                <StatusChip
                  label={sessionStatusLabel(selectedSession.status)}
                  tone={toneOf(COUNT_SESSION_STATUS_TONE, selectedSession.status)}
                  dot
                />
                {selectedSession.critical_abs_threshold != null ? (
                  <span
                    className="text-xs text-text-secondary"
                    data-testid="inv-cc-session-threshold"
                  >
                    {t("controls.sessions.criticalThresholdOfSession", {
                      value: formatNumber(selectedSession.critical_abs_threshold, {
                        maximumFractionDigits: 6,
                      }),
                    })}
                  </span>
                ) : null}
                {selectedSession.status === "draft" ? (
                  <Button
                    variant="outline"
                    className="h-11"
                    disabled={saving}
                    onClick={() => transitionSession("counting")}
                  >
                    {t("controls.sessions.startCounting")}
                  </Button>
                ) : null}
                {selectedSession.status === "counting" ? (
                  <Button
                    variant="outline"
                    className="h-11"
                    disabled={saving}
                    onClick={() => transitionSession("submitted")}
                  >
                    {t("controls.sessions.submit")}
                  </Button>
                ) : null}
                {selectedSession.status === "submitted" ? (
                  <Button
                    variant="outline"
                    className="h-11"
                    // The count is approved once every variance has its reviewer's approval (the
                    // server refuses it otherwise, and also requires the reviewer's note, which
                    // the line approval always records); the pending count is shown above the lines.
                    disabled={saving || pendingApprovals > 0}
                    onClick={() => transitionSession("approved")}
                  >
                    {t("controls.sessions.approve")}
                  </Button>
                ) : null}
                {selectedSession.status === "approved" ? (
                  <Button
                    className="h-11"
                    disabled={saving}
                    onClick={() =>
                      void withSave(async () => {
                        await postInventoryCountSession({
                          session_id: selectedSession.id,
                          expected_row_version: selectedSession.row_version,
                          actor_id: info?.user_id ?? null,
                        });
                      })
                    }
                  >
                    {t("controls.sessions.post")}
                  </Button>
                ) : null}
              </>
            ) : null}
          </div>
          {selectedSession ? (
            <p className="mt-2 text-xs text-text-muted">
              {t("controls.sessions.summary", {
                warehouse: warehouseLabel(selectedSession.warehouse_id),
                created: formatDateTime(selectedSession.created_at),
              })}{" "}
              {t(`controls.sessions.next.${selectedSession.status}`, { defaultValue: "" })}
            </p>
          ) : null}
        </section>
      </PermissionGate>

      {selectedSession ? (
        <section className="rounded-md border p-4">
          <h3 className="mb-1 text-sm font-semibold">{t("controls.lines.title")}</h3>
          <p className="mb-3 text-xs text-text-muted">
            {pendingApprovals > 0
              ? t("controls.lines.pendingCount", { count: pendingApprovals })
              : t("controls.lines.hint")}
          </p>
          {selectedSession.status === "draft" || selectedSession.status === "counting" ? (
            <PermissionGate permission={P.INV_COUNT}>
              <div className="grid gap-2 md:grid-cols-5">
                <div className="space-y-1">
                  <Label htmlFor="inv-cl-article" className="text-xs">
                    {t("controls.lines.article")}
                  </Label>
                  <Select
                    value={String(lineArticleId)}
                    onValueChange={(v) => setLineArticleId(Number(v))}
                  >
                    <SelectTrigger id="inv-cl-article">
                      <SelectValue placeholder={t("procurement.requisitionForm.selectArticle")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="0">
                        {t("procurement.requisitionForm.selectArticle")}
                      </SelectItem>
                      {articles.map((a) => (
                        <SelectItem key={a.id} value={String(a.id)}>
                          {a.article_code} - {a.article_name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1">
                  <Label htmlFor="inv-cl-location" className="text-xs">
                    {t("controls.lines.location")}
                  </Label>
                  <Select
                    value={String(lineLocationId)}
                    onValueChange={(v) => setLineLocationId(Number(v))}
                  >
                    <SelectTrigger id="inv-cl-location">
                      <SelectValue placeholder={t("adjustStock.placeholders.location")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="0">{t("adjustStock.placeholders.location")}</SelectItem>
                      {locations
                        .filter((l) => l.warehouse_id === selectedSession.warehouse_id)
                        .map((l) => (
                          <SelectItem key={l.id} value={String(l.id)}>
                            {l.warehouse_code}/{l.code}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1">
                  <Label htmlFor="inv-cl-qty" className="text-xs">
                    {t("controls.lines.countedQty")}
                  </Label>
                  <DecimalField
                    id="inv-cl-qty"
                    decimals={3}
                    min={0}
                    value={lineCountQty}
                    onValueChange={(v) => setLineCountQty(v ?? 0)}
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="inv-cl-reason" className="text-xs">
                    {t("controls.lines.varianceReason")}
                  </Label>
                  <Select
                    value={lineVarianceReasonCode || "__none__"}
                    onValueChange={(v) => setLineVarianceReasonCode(v === "__none__" ? "" : v)}
                  >
                    <SelectTrigger id="inv-cl-reason">
                      <SelectValue placeholder={t("controls.lines.noVarianceReason")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">
                        {t("controls.lines.noVarianceReason")}
                      </SelectItem>
                      {varianceReasons.options.map((reason) => (
                        <SelectItem key={reason.id} value={reason.code}>
                          {varianceReasons.labelOf(reason.code)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex items-end">
                  <Button
                    className="h-11"
                    aria-describedby={lineBlocker ? "inv-cl-blocker" : undefined}
                    disabled={saving || lineBlocker !== null}
                    onClick={() =>
                      void withSave(async () => {
                        // Entered again against the version shown, so an entry made meanwhile
                        // elsewhere is refused instead of overwritten (AU-68).
                        const current = lines.find(
                          (line) =>
                            line.article_id === lineArticleId &&
                            line.location_id === lineLocationId,
                        );
                        await upsertInventoryCountLine({
                          session_id: selectedSession.id,
                          article_id: lineArticleId,
                          location_id: lineLocationId,
                          counted_qty: lineCountQty,
                          variance_reason_code: lineVarianceReasonCode || null,
                          expected_row_version: current?.row_version ?? null,
                        });
                      })
                    }
                  >
                    {t("controls.lines.record")}
                  </Button>
                </div>
              </div>
              {lineBlocker ? (
                <p id="inv-cl-blocker" className="mt-1 text-xs text-text-muted">
                  {lineBlocker}
                </p>
              ) : null}
            </PermissionGate>
          ) : null}
          <div className="mt-3">
            {linesError ? (
              <ErrorState error={linesError} onRetry={() => void refreshCountLines()} />
            ) : (
              <DataTable
                columns={lineColumns}
                data={lines}
                searchable={false}
                isLoading={loading}
              />
            )}
          </div>
          {selectedSession.status === "posted" ? (
            <PermissionGate permission={P.INV_MANAGE}>
              <div className="mt-3 space-y-1">
                <Label htmlFor="inv-cc-reversal" className="text-xs">
                  {t("controls.reversal.reason")}
                </Label>
                <div className="flex gap-2">
                  <Input
                    id="inv-cc-reversal"
                    value={sessionReversalReason}
                    onChange={(e) => setSessionReversalReason(e.target.value)}
                    placeholder={t("controls.reversal.placeholder", {
                      count: REVERSAL_REASON_MIN_LENGTH,
                    })}
                  />
                  <Button
                    variant="destructive"
                    className="h-11"
                    disabled={
                      saving ||
                      [...sessionReversalReason.trim()].length < REVERSAL_REASON_MIN_LENGTH
                    }
                    onClick={() => setReverseConfirmOpen(true)}
                  >
                    {t("controls.reversal.action")}
                  </Button>
                </div>
              </div>
            </PermissionGate>
          ) : null}
        </section>
      ) : null}

      <section className="rounded-md border p-4">
        <h3 className="mb-1 text-sm font-semibold">{t("controls.reconciliation.title")}</h3>
        <p className="mb-1 text-xs text-text-muted">{t("controls.reconciliation.hint")}</p>
        <p className="mb-3 text-xs text-text-muted">
          {t("controls.reconciliation.breakFromSettings")}{" "}
          <Link to={INVENTORY_SETTINGS_PATH} className="underline underline-offset-2">
            {t("controls.setInSettings")}
          </Link>
        </p>
        <div className="flex flex-wrap items-end gap-2">
          <PermissionGate permission={P.ERP_RECONCILE}>
            <Button
              className="h-11"
              disabled={saving}
              onClick={() =>
                void withSave(async () => {
                  await runInventoryReconciliation({
                    actor_id: info?.user_id ?? null,
                  });
                })
              }
            >
              {t("controls.reconciliation.run")}
            </Button>
          </PermissionGate>
          <div className="space-y-1">
            <Label htmlFor="inv-recon-run" className="text-xs">
              {t("controls.reconciliation.run_label")}
            </Label>
            <Select
              value={String(selectedRunId ?? 0)}
              onValueChange={(v) => setSelectedRunId(Number(v) || null)}
            >
              <SelectTrigger id="inv-recon-run" className="w-80">
                <SelectValue placeholder={t("controls.reconciliation.selectRun")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0">{t("controls.reconciliation.selectRun")}</SelectItem>
                {runs.map((run) => (
                  <SelectItem key={run.id} value={String(run.id)}>
                    {t("controls.reconciliation.runOption", {
                      code: run.run_code,
                      date: formatDateTime(run.started_at),
                      count: run.drift_rows,
                    })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <div className="mt-3">
          {findingsError ? (
            <ErrorState
              error={findingsError}
              onRetry={() => {
                const id = selectedRunId;
                setSelectedRunId(null);
                window.setTimeout(() => setSelectedRunId(id), 0);
              }}
            />
          ) : (
            <DataTable
              columns={findingColumns}
              data={findings}
              searchable={false}
              isLoading={loading}
            />
          )}
        </div>
      </section>

      <section className="rounded-md border p-4">
        <h3 className="mb-1 text-sm font-semibold">{t("controls.trace.title")}</h3>
        <p className="mb-3 text-xs text-text-muted">
          {t("controls.trace.hint", { count: COUNT_TRACE_LIMIT })}
        </p>
        <DataTable columns={traceColumns} data={traceRows} searchable={false} isLoading={loading} />
      </section>

      {/* Variance approval: the selected line, with mandatory reviewer evidence. */}
      <Dialog
        open={approvalLine !== null}
        onOpenChange={(open) => {
          if (!open && !saving) setApprovalLine(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("controls.approval.title")}</DialogTitle>
            <DialogDescription>
              {approvalLine
                ? `${t("controls.approval.description", {
                    article: `${approvalLine.article_code} — ${approvalLine.article_name}`,
                    location: approvalLine.location_code,
                    system: qty(approvalLine.system_qty),
                    counted: qty(approvalLine.counted_qty),
                    variance: qty(approvalLine.variance_qty),
                  })} ${t(
                    approvalLine.is_critical === 1
                      ? "controls.approval.aboveTolerance"
                      : "controls.approval.withinTolerance",
                    {
                      counter:
                        approvalLine.counted_by_name ?? t("controls.lines.counterNotRecorded"),
                    },
                  )}`
                : ""}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-1">
            <Label htmlFor="inv-cl-evidence">{t("controls.approval.evidence")}</Label>
            <Textarea
              id="inv-cl-evidence"
              rows={3}
              value={approvalEvidence}
              onChange={(e) => setApprovalEvidence(e.target.value)}
              placeholder={t("controls.approval.evidencePlaceholder")}
            />
          </div>
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={saving}
              onClick={() => setApprovalLine(null)}
            >
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={saving || !approvalEvidence.trim()}
              onClick={() => void approveSelectedLine()}
            >
              {t("controls.approval.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={reverseConfirmOpen}
        onOpenChange={setReverseConfirmOpen}
        title={t("controls.reversal.confirmTitle")}
        description={t("controls.reversal.confirmDescription", {
          code: selectedSession?.session_code ?? "",
        })}
        confirmLabel={t("controls.reversal.action")}
        destructive
        onConfirm={async () => {
          if (!selectedSession) return;
          const ok = await withSave(async () => {
            await reverseInventoryCountSession({
              session_id: selectedSession.id,
              expected_row_version: selectedSession.row_version,
              reason: sessionReversalReason.trim(),
              actor_id: info?.user_id ?? null,
            });
          });
          if (!ok) throw new Error("reversal failed");
          setSessionReversalReason("");
        }}
      />
    </div>
  );
}

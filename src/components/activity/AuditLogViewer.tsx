/**
 * Audit log (Settings › Activity & audit): the immutable trail of security and compliance
 * actions. FR/EN labels, plain filter names, details as readable lines (hashes under
 * "technical details"), load errors with retry, paging that actually reloads, export through
 * the Save dialog.
 */
import { Download, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AuditChainCheck } from "@/components/activity/AuditChainCheck";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
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
import { saveBlobWithDialog } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import {
  type AuditEventDetail,
  type AuditEventSummary,
  type AuditFilter,
  exportAuditLog,
  getAuditEvent,
  listAuditEvents,
} from "@/services/activity-service";
import { getSessionInfo } from "@/services/auth-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

import {
  AUDIT_RESULT_OPTIONS,
  AUDIT_RETENTION_OPTIONS,
  authContextLabel,
  eventLabel,
  humanizeCode,
  payloadLines,
  resultLabel,
  resultTone,
  retentionLabel,
} from "./activity-labels";

const PAGE_SIZE = 40;
const ANY = "__any__";

interface AuditLogViewerProps {
  className?: string;
}

export function AuditLogViewer({ className }: AuditLogViewerProps) {
  const { t } = useTranslation("activity");
  const { t: tShell } = useTranslation("shell");
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();

  const [items, setItems] = useState<AuditEventSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [detailsMap, setDetailsMap] = useState<Record<number, AuditEventDetail>>({});
  const [detailErrors, setDetailErrors] = useState<Record<number, unknown>>({});
  const [detailLoadingMap, setDetailLoadingMap] = useState<Record<number, boolean>>({});
  const [currentUserId, setCurrentUserId] = useState<number | null>(null);

  const [actionCode, setActionCode] = useState("");
  const [result, setResult] = useState("");
  const [actorId, setActorId] = useState("");
  const [retentionClass, setRetentionClass] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [mySecurityOnly, setMySecurityOnly] = useState(false);
  const [applied, setApplied] = useState<AuditFilter>({ limit: PAGE_SIZE, offset: 0 });
  const offset = applied.offset ?? 0;

  const [exportReason, setExportReason] = useState("");
  const [exporting, setExporting] = useState(false);

  const draftFilter = useMemo<AuditFilter>(() => {
    const out: AuditFilter = { limit: PAGE_SIZE, offset: 0 };
    if (actionCode.trim()) out.action_code = actionCode.trim();
    if (result) out.result = result;
    if (retentionClass) out.retention_class = retentionClass;
    if (dateFrom) out.date_from = `${dateFrom}T00:00:00Z`;
    if (dateTo) out.date_to = `${dateTo}T23:59:59Z`;
    const forcedActor = mySecurityOnly ? currentUserId : null;
    if (typeof forcedActor === "number") {
      out.actor_id = forcedActor;
    } else if (actorId.trim()) {
      const parsed = Number(actorId);
      if (!Number.isNaN(parsed)) out.actor_id = parsed;
    }
    return out;
  }, [
    actionCode,
    actorId,
    currentUserId,
    dateFrom,
    dateTo,
    mySecurityOnly,
    result,
    retentionClass,
  ]);

  const visibleItems = useMemo(() => {
    if (!mySecurityOnly) return items;
    return items.filter((row) => row.auth_context === "step_up" || row.auth_context === "password");
  }, [items, mySecurityOnly]);

  const loadData = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setItems(await listAuditEvents(applied));
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, [applied]);

  useEffect(() => {
    void getSessionInfo()
      .then((session) => {
        setCurrentUserId(typeof session.user_id === "number" ? session.user_id : null);
      })
      .catch(() => setCurrentUserId(null));
  }, []);

  // Applied filter or page changed: reload (paging used to change nothing on screen).
  useEffect(() => {
    void loadData();
  }, [loadData]);

  const openRow = async (row: AuditEventSummary) => {
    setExpandedId((prev) => (prev === row.id ? null : row.id));
    if (detailsMap[row.id]) return;
    setDetailLoadingMap((prev) => ({ ...prev, [row.id]: true }));
    try {
      const detail = await getAuditEvent(row.id);
      setDetailsMap((prev) => ({ ...prev, [row.id]: detail }));
      setDetailErrors((prev) => {
        const next = { ...prev };
        delete next[row.id];
        return next;
      });
    } catch (err) {
      setDetailErrors((prev) => ({ ...prev, [row.id]: err }));
    } finally {
      setDetailLoadingMap((prev) => ({ ...prev, [row.id]: false }));
    }
  };

  const runExport = async () => {
    setExporting(true);
    try {
      let exported: Awaited<ReturnType<typeof exportAuditLog>>;
      try {
        exported = await exportAuditLog({ filter: applied, export_reason: exportReason.trim() });
      } catch {
        return; // The IPC toast reports it.
      }
      try {
        const path = await saveBlobWithDialog(
          new Blob([JSON.stringify(exported.rows_json, null, 2)], {
            type: "application/json;charset=utf-8",
          }),
          `audit-export-${exported.event_export_run_id}.json`,
          { title: t("audit.export.action"), filters: [{ name: "JSON", extensions: ["json"] }] },
        );
        pushAppToast({
          title: t("audit.export.saved", { count: exported.row_count }),
          description: path,
          variant: "success",
        });
        setExportReason("");
        await loadData();
      } catch (err) {
        if (isExportCancelled(err)) return;
        pushAppToast({
          title: t("audit.export.failed"),
          description: describeAppError(err).title,
          variant: "destructive",
        });
      }
    } finally {
      setExporting(false);
    }
  };

  const selectFilter = (
    id: string,
    label: string,
    value: string,
    onChange: (v: string) => void,
    options: readonly string[],
    optionLabel: (code: string) => string,
  ) => (
    <div className="space-y-1">
      <Label htmlFor={id} className="text-xs text-text-muted">
        {label}
      </Label>
      <Select value={value || ANY} onValueChange={(v) => onChange(v === ANY ? "" : v)}>
        <SelectTrigger id={id} aria-label={label} className="h-10">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ANY}>{t("filters.any")}</SelectItem>
          {options.map((o) => (
            <SelectItem key={o} value={o}>
              {optionLabel(o)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );

  return (
    <Card className={className}>
      <CardHeader className="flex flex-row items-center justify-between gap-2 pb-3">
        <CardTitle className="text-base">{t("tabs.audit")}</CardTitle>
        <RefreshButton onRefresh={() => void loadData()} loading={loading} />
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <div className="space-y-1">
            <Label htmlFor="audit-filter-action-code" className="text-xs text-text-muted">
              {t("audit.filters.action")}
            </Label>
            <Input
              id="audit-filter-action-code"
              className="h-10"
              placeholder={t("audit.filters.actionHint")}
              value={actionCode}
              onChange={(e) => setActionCode(e.target.value)}
            />
          </div>
          {selectFilter(
            "audit-filter-result",
            t("audit.filters.result"),
            result,
            setResult,
            AUDIT_RESULT_OPTIONS,
            (c) => resultLabel(t, c),
          )}
          <div className="space-y-1">
            <Label htmlFor="audit-filter-actor-id" className="text-xs text-text-muted">
              {t("audit.filters.userNumber")}
            </Label>
            <Input
              id="audit-filter-actor-id"
              inputMode="numeric"
              className="h-10"
              disabled={mySecurityOnly}
              value={actorId}
              onChange={(e) => setActorId(e.target.value)}
            />
          </div>
          {selectFilter(
            "audit-filter-retention-class",
            t("audit.filters.retention"),
            retentionClass,
            setRetentionClass,
            AUDIT_RETENTION_OPTIONS,
            (c) => retentionLabel(t, c),
          )}
          <div className="space-y-1">
            <Label htmlFor="audit-filter-date-from" className="text-xs text-text-muted">
              {t("filters.dateFrom")}
            </Label>
            <Input
              id="audit-filter-date-from"
              type="date"
              className="h-10"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor="audit-filter-date-to" className="text-xs text-text-muted">
              {t("filters.dateTo")}
            </Label>
            <Input
              id="audit-filter-date-to"
              type="date"
              className="h-10"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
            />
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button className="h-10" onClick={() => setApplied({ ...draftFilter, offset: 0 })}>
            {t("filters.apply")}
          </Button>
          <Button
            className="h-10"
            variant="outline"
            onClick={() => {
              setActionCode("");
              setResult("");
              setActorId("");
              setRetentionClass("");
              setDateFrom("");
              setDateTo("");
              setMySecurityOnly(false);
              setApplied({ limit: PAGE_SIZE, offset: 0 });
            }}
          >
            {t("filters.reset")}
          </Button>
          <Button
            className="h-10 gap-1.5"
            variant={mySecurityOnly ? "default" : "outline"}
            aria-pressed={mySecurityOnly}
            onClick={() => setMySecurityOnly((prev) => !prev)}
          >
            <ShieldCheck className="h-4 w-4" aria-hidden />
            {t("audit.mySecurityEvents")}
          </Button>
        </div>

        {can(P.ADM_AUDIT) && <AuditChainCheck />}

        {can(P.LOG_EXPORT) && (
          <div className="space-y-2 rounded-md border border-surface-border p-3">
            <p className="text-xs text-text-muted">{t("audit.export.help")}</p>
            <div className="flex flex-wrap items-end gap-2">
              <div className="min-w-[240px] flex-1 space-y-1">
                <Label htmlFor="audit-export-reason" className="text-xs text-text-muted">
                  {t("audit.export.reason")}
                </Label>
                <Textarea
                  id="audit-export-reason"
                  className="min-h-[64px]"
                  value={exportReason}
                  onChange={(e) => setExportReason(e.target.value)}
                />
              </div>
              <Button
                className="h-10 gap-1.5"
                disabled={!exportReason.trim() || exporting}
                onClick={() => void runExport()}
              >
                <Download className="h-4 w-4" aria-hidden />
                {t("audit.export.action")}
              </Button>
            </div>
          </div>
        )}

        {loadError != null && !loading ? (
          <ErrorState
            error={loadError}
            title={t("audit.loadError")}
            onRetry={() => void loadData()}
          />
        ) : loading && items.length === 0 ? (
          <LoadingState />
        ) : visibleItems.length === 0 ? (
          <EmptyState title={t("audit.empty")} />
        ) : (
          <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
            {visibleItems.map((row) => {
              const detail = detailsMap[row.id];
              const lines = payloadLines(detail?.details_json);
              return (
                <li key={row.id} className="px-3 py-2 text-sm">
                  <button
                    type="button"
                    aria-expanded={expandedId === row.id}
                    className="grid min-h-10 w-full grid-cols-[1fr_auto] items-center gap-2 text-start focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring sm:grid-cols-[1fr_auto_auto_auto_auto]"
                    onClick={() => void openRow(row)}
                  >
                    <span className="min-w-0">
                      <span className="block truncate font-medium text-text-primary">
                        {eventLabel(t, row.action_code)}
                      </span>
                      <span className="block truncate font-mono text-[11px] text-text-muted">
                        {row.action_code}
                      </span>
                    </span>
                    <span className="text-xs text-text-secondary">
                      {row.actor_username ?? t("feed.system")}
                    </span>
                    <StatusChip label={authContextLabel(t, row.auth_context)} tone="neutral" />
                    <StatusChip
                      label={resultLabel(t, row.result)}
                      tone={resultTone(row.result)}
                      dot
                    />
                    <time className="text-xs text-text-muted" dateTime={row.happened_at}>
                      {formatDateTime(row.happened_at)}
                    </time>
                  </button>
                  {expandedId === row.id ? (
                    <div className="mt-2 space-y-2 rounded-md bg-surface-2 p-3 text-xs">
                      {detailLoadingMap[row.id] ? <LoadingState className="py-2" /> : null}
                      {detailErrors[row.id] != null ? (
                        <p role="alert" className="text-tone-danger-fg">
                          {describeAppError(detailErrors[row.id]).title}
                        </p>
                      ) : null}
                      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
                        <dt className="text-text-muted">{t("audit.detail.target")}</dt>
                        <dd>
                          {row.target_type ? humanizeCode(row.target_type) : "—"}
                          {row.target_id ? ` · ${row.target_id}` : ""}
                        </dd>
                        <dt className="text-text-muted">{t("audit.detail.retention")}</dt>
                        <dd>{retentionLabel(t, row.retention_class)}</dd>
                        {lines.map((l, i) => (
                          <div key={`${l.key}-${i}`} className="contents">
                            <dt className="text-text-muted">{l.key}</dt>
                            <dd className="break-words">{l.value}</dd>
                          </div>
                        ))}
                      </dl>
                      {row.from_other_device ? (
                        <p className="text-text-muted" data-testid="history-from-other-device">
                          {tShell("sync.history.fromOtherDevice")}
                        </p>
                      ) : null}
                      {detail ? (
                        <details>
                          <summary className="cursor-pointer select-none text-text-muted">
                            {t("audit.detail.technical")}
                          </summary>
                          <p className="mt-1 break-all font-mono">
                            {t("audit.detail.before")}: {detail.before_hash ?? "—"}
                          </p>
                          <p className="break-all font-mono">
                            {t("audit.detail.after")}: {detail.after_hash ?? "—"}
                          </p>
                        </details>
                      ) : null}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}

        <div className="flex flex-wrap items-center justify-end gap-2">
          {items.length > 0 ? (
            <span className="text-xs text-text-muted" role="status">
              {t("pager.range", { from: offset + 1, to: offset + items.length })}
            </span>
          ) : null}
          <Button
            variant="outline"
            className="h-10"
            disabled={offset === 0 || loading}
            onClick={() =>
              setApplied((cur) => ({ ...cur, offset: Math.max(0, (cur.offset ?? 0) - PAGE_SIZE) }))
            }
          >
            {t("pager.previous")}
          </Button>
          <Button
            variant="outline"
            className="h-10"
            disabled={items.length < PAGE_SIZE || loading}
            onClick={() => setApplied((cur) => ({ ...cur, offset: (cur.offset ?? 0) + PAGE_SIZE }))}
          >
            {t("pager.next")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

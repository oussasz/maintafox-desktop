import type { ColumnDef } from "@tanstack/react-table";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  PM_EXECUTION_RESULT_TONE,
  PM_FINDING_SEVERITY_TONE,
  PM_PLAN_LIFECYCLE_TONE,
  PM_VERSION_STATUS_TONE,
  toneOf,
} from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import type { PmExecutionRow, PmOccurrenceRow, PmPlanRow } from "@/services/pm-service";
import { PM_RECURRING_FINDINGS_WINDOW_DAYS } from "@/stores/pm-store";
import type {
  PmFinding,
  PmGovernanceKpiReport,
  PmPlanVersion,
  PmPlanningReadinessProjection,
  PmRateKpi,
  PmRecurringFinding,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  PmAssetCell,
  PmDueLabel,
  PmOccurrenceStatus,
  PmTimingChip,
  formatPmAsset,
} from "./pm-cells";
import {
  PM_READINESS_BLOCKERS,
  PUBLISHABLE_PLAN_LIFECYCLES,
  allowedOccurrenceTargets,
  allowedPlanLifecycleTargets,
} from "./pm-labels";
import { canLogExecution } from "./pm-model";
import { usePmFindingTypeLabel } from "./use-pm-finding-type-label";

interface PmPlanDetailDialogProps {
  open: boolean;
  plan: PmPlanRow | null;
  versions: PmPlanVersion[];
  occurrences: PmOccurrenceRow[];
  executions: PmExecutionRow[];
  findingsByExecutionId: Record<number, PmFinding[]>;
  recurringFindings: PmRecurringFinding[];
  planningReadiness: PmPlanningReadinessProjection | null;
  governanceKpis: PmGovernanceKpiReport | null;
  saving?: boolean;
  detailLoading?: boolean;
  detailError?: unknown;
  onRetryDetail: () => void;
  onClose: () => void;
  onEditPlan: () => void;
  onDeletePlan: () => void;
  onLifecycle: (next: string) => void;
  onNewVersion: () => void;
  onEditVersion: (version: PmPlanVersion) => void;
  onPublishVersion: (version: PmPlanVersion) => void;
  onDeleteVersion: (version: PmPlanVersion) => void;
  onTransitionOccurrence: (occurrence: PmOccurrenceRow) => void;
  onExecuteOccurrence: (occurrence: PmOccurrenceRow) => void;
  onLoadFindings: (executionId: number) => void;
  /** "Revise the plan" from a finding (draft version); resolves when created. */
  onReviseFromFinding: (finding: PmFinding) => Promise<void>;
  /** Occurrence to show first (deep link `/pm?occurrence=<id>`). */
  focusOccurrenceId?: number | null;
}

const KNOWN_BLOCKERS = new Set<string>(PM_READINESS_BLOCKERS);

export function PmPlanDetailDialog({
  open,
  plan,
  versions,
  occurrences,
  executions,
  findingsByExecutionId,
  recurringFindings,
  planningReadiness,
  governanceKpis,
  saving,
  detailLoading,
  detailError,
  onRetryDetail,
  onClose,
  onEditPlan,
  onDeletePlan,
  onLifecycle,
  onNewVersion,
  onEditVersion,
  onPublishVersion,
  onDeleteVersion,
  onTransitionOccurrence,
  onExecuteOccurrence,
  onLoadFindings,
  onReviseFromFinding,
  focusOccurrenceId,
}: PmPlanDetailDialogProps) {
  const { t } = useTranslation("pm");
  const { formatDate, formatDateTime, formatNumber, formatPercent } = useFormatters();
  const findingTypeLabel = usePmFindingTypeLabel();
  const [tab, setTab] = useState("overview");
  const [findingsFor, setFindingsFor] = useState<number | null>(null);
  const [revising, setRevising] = useState<number | null>(null);

  useEffect(() => {
    if (focusOccurrenceId != null) setTab("occurrences");
  }, [focusOccurrenceId]);
  const focused =
    focusOccurrenceId != null
      ? (occurrences.find((o) => o.id === focusOccurrenceId) ?? null)
      : null;

  const lifecycleTargets = plan ? allowedPlanLifecycleTargets(plan.lifecycle_status) : [];

  const versionColumns: ColumnDef<PmPlanVersion>[] = useMemo(
    () => [
      {
        accessorKey: "version_no",
        header: t("table.version"),
        cell: ({ row }) => `v${row.original.version_no}`,
      },
      {
        accessorKey: "status",
        header: t("table.status"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`versionStatus.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
            tone={toneOf(PM_VERSION_STATUS_TONE, row.original.status)}
            dot
          />
        ),
      },
      {
        id: "window",
        header: t("table.window"),
        cell: ({ row }) =>
          `${formatDate(row.original.effective_from)}${
            row.original.effective_to ? ` → ${formatDate(row.original.effective_to)}` : ""
          }`,
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => {
          const v = row.original;
          const isDraft = v.status === "draft";
          const publishBlocked = !plan || !PUBLISHABLE_PLAN_LIFECYCLES.has(plan.lifecycle_status);
          return (
            <div className="flex flex-wrap gap-1">
              <PermissionGate permission={P.PM_EDIT}>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!isDraft || saving}
                  title={!isDraft ? t("hints.editVersionDraftOnly") : undefined}
                  onClick={() => onEditVersion(v)}
                >
                  {t("actions.editVersion")}
                </Button>
              </PermissionGate>
              <PermissionGate permission={P.PM_EDIT}>
                <Button
                  size="sm"
                  disabled={!isDraft || saving || publishBlocked}
                  title={
                    !isDraft
                      ? t("hints.publishDraftOnly")
                      : publishBlocked
                        ? t("hints.publishBlockedByLifecycle")
                        : undefined
                  }
                  onClick={() => onPublishVersion(v)}
                >
                  {t("actions.publish")}
                </Button>
              </PermissionGate>
              <PermissionGate permission={P.PM_DELETE}>
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={!isDraft || saving}
                  title={!isDraft ? t("hints.deleteVersionDraftOnly") : undefined}
                  onClick={() => onDeleteVersion(v)}
                >
                  {t("actions.deleteVersion")}
                </Button>
              </PermissionGate>
            </div>
          );
        },
      },
    ],
    [t, formatDate, saving, plan, onEditVersion, onPublishVersion, onDeleteVersion],
  );

  const occurrenceColumns: ColumnDef<PmOccurrenceRow>[] = useMemo(
    () => [
      {
        id: "due",
        header: t("table.due"),
        cell: ({ row }) => <PmDueLabel occurrence={row.original} />,
      },
      {
        id: "asset",
        header: t("table.asset"),
        cell: ({ row }) => (
          <PmAssetCell code={row.original.asset_code} name={row.original.asset_name} />
        ),
      },
      {
        accessorKey: "status",
        header: t("table.status"),
        cell: ({ row }) => <PmOccurrenceStatus occurrence={row.original} />,
      },
      {
        id: "wo",
        header: t("table.workOrder"),
        cell: ({ row }) =>
          row.original.linked_work_order_code ? (
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.linked_work_order_code}
              entityId={row.original.linked_work_order_id}
            />
          ) : (
            <span className="text-xs text-text-muted">{t("labels.none")}</span>
          ),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => {
          const occ = row.original;
          return (
            <PermissionGate permission={P.PM_EDIT}>
              <div className="flex flex-wrap gap-1">
                {allowedOccurrenceTargets(occ.status).length > 0 ? (
                  <Button size="sm" variant="outline" onClick={() => onTransitionOccurrence(occ)}>
                    {t("actions.transitionOccurrence")}
                  </Button>
                ) : null}
                {canLogExecution(occ) ? (
                  <Button size="sm" variant="secondary" onClick={() => onExecuteOccurrence(occ)}>
                    {t("actions.logExecution")}
                  </Button>
                ) : null}
              </div>
            </PermissionGate>
          );
        },
      },
    ],
    [t, onTransitionOccurrence, onExecuteOccurrence],
  );

  const executionColumns: ColumnDef<PmExecutionRow>[] = useMemo(
    () => [
      {
        accessorKey: "executed_at",
        header: t("table.executedAt"),
        cell: ({ row }) => formatDateTime(row.original.executed_at),
      },
      {
        accessorKey: "execution_result",
        header: t("table.result"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`executionResult.${row.original.execution_result}`, {
              defaultValue: row.original.execution_result,
            })}
            tone={toneOf(PM_EXECUTION_RESULT_TONE, row.original.execution_result)}
          />
        ),
      },
      {
        id: "timing",
        header: t("table.timing"),
        cell: ({ row }) =>
          row.original.timing_status ? (
            <PmTimingChip timing={row.original.timing_status} />
          ) : (
            <span className="text-xs text-text-muted">{t("labels.none")}</span>
          ),
      },
      {
        id: "wo",
        header: t("table.workOrder"),
        cell: ({ row }) =>
          row.original.work_order_code ? (
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.work_order_code}
              entityId={row.original.work_order_id}
            />
          ) : (
            <span className="text-xs text-text-muted">{t("labels.none")}</span>
          ),
      },
      {
        id: "notes",
        header: t("table.notes"),
        cell: ({ row }) => (
          <span className="block max-w-[260px] whitespace-pre-line text-xs">
            {row.original.notes || t("labels.none")}
          </span>
        ),
      },
      {
        id: "duration",
        header: t("table.actualDuration"),
        cell: ({ row }) =>
          row.original.actual_duration_hours != null
            ? formatNumber(row.original.actual_duration_hours)
            : t("labels.none"),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => (
          <Button
            size="sm"
            variant="outline"
            aria-pressed={findingsFor === row.original.id}
            onClick={() => {
              setFindingsFor(row.original.id);
              onLoadFindings(row.original.id);
            }}
          >
            {t("actions.viewFindings")}
          </Button>
        ),
      },
    ],
    [t, formatDateTime, formatNumber, onLoadFindings, findingsFor],
  );

  if (!plan) return null;

  const findings = findingsFor != null ? (findingsByExecutionId[findingsFor] ?? []) : [];
  const asset = formatPmAsset(plan.asset_code, plan.asset_name);
  const pct = (kpi: PmRateKpi) => (kpi.value_pct != null ? formatPercent(kpi.value_pct) : "—");
  const ratio = (kpi: PmRateKpi) =>
    t("kpi.sample", {
      numerator: formatNumber(kpi.numerator),
      denominator: formatNumber(kpi.denominator),
    });

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent
        className="flex max-h-[90vh] max-w-5xl flex-col gap-0 p-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className="border-b px-6 py-4">
          <DialogTitle className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-base">{plan.code}</span>
            <span className="font-normal">{plan.title}</span>
          </DialogTitle>
          <DialogDescription className="sr-only">{t("sections.overview")}</DialogDescription>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <StatusChip
              label={t(`lifecycle.${plan.lifecycle_status}`, {
                defaultValue: plan.lifecycle_status,
              })}
              tone={toneOf(PM_PLAN_LIFECYCLE_TONE, plan.lifecycle_status)}
              size="md"
            />
            <StatusChip
              label={t(`strategy.${plan.strategy_type}`, { defaultValue: plan.strategy_type })}
              tone="neutral"
              size="md"
            />
            {asset ? <StatusChip label={asset} tone="info" size="md" /> : null}
            {plan.source_decision_to_review ? (
              <StatusChip
                label={t("sourceDecision.toReview")}
                title={t("sourceDecision.toReviewHint")}
                tone="warning"
                size="md"
              />
            ) : null}
            {plan.assigned_team_label || plan.assigned_team_name ? (
              <StatusChip
                label={plan.assigned_team_label || plan.assigned_team_name || ""}
                tone="neutral"
                size="md"
              />
            ) : null}
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          {detailError != null ? (
            <ErrorState
              error={detailError}
              title={t("page.detailLoadFailed")}
              onRetry={onRetryDetail}
              retrying={detailLoading ?? false}
              className="mb-4"
            />
          ) : null}
          {detailLoading ? <LoadingState /> : null}
          <Tabs value={tab} onValueChange={setTab}>
            <TabsList className="mb-4 flex h-auto flex-wrap">
              <TabsTrigger value="overview">{t("sections.overview")}</TabsTrigger>
              <TabsTrigger value="versions">{t("sections.versions")}</TabsTrigger>
              <TabsTrigger value="occurrences">{t("sections.occurrences")}</TabsTrigger>
              <TabsTrigger value="executions">{t("sections.executions")}</TabsTrigger>
              <TabsTrigger value="governance">{t("sections.governance")}</TabsTrigger>
            </TabsList>

            <TabsContent value="overview" className="space-y-4">
              <dl className="grid gap-x-6 gap-y-2 rounded-lg border p-4 text-sm sm:grid-cols-[max-content_1fr]">
                <dt className="text-text-muted">{t("fields.scopeType")}</dt>
                <dd>
                  {t(`scope.${plan.asset_scope_type}`, { defaultValue: plan.asset_scope_type })}
                </dd>
                <dt className="text-text-muted">{t("fields.asset")}</dt>
                <dd>
                  {plan.asset_scope_type === "equipment" ? (
                    <PmAssetCell code={plan.asset_code} name={plan.asset_name} />
                  ) : (
                    <span>
                      {t("labels.scopeOf", {
                        scope: t(`scope.${plan.asset_scope_type}`, {
                          defaultValue: plan.asset_scope_type,
                        }),
                        value: plan.asset_scope_label ?? t("labels.none"),
                      })}
                      <span className="block text-xs text-text-muted">{t("hints.fanOut")}</span>
                    </span>
                  )}
                </dd>
                <dt className="text-text-muted">{t("fields.criticality")}</dt>
                <dd>{plan.criticality_label || t("labels.none")}</dd>
                <dt className="text-text-muted">{t("fields.requiresShutdown")}</dt>
                <dd>{plan.requires_shutdown === 1 ? t("labels.yes") : t("labels.no")}</dd>
                <dt className="text-text-muted">{t("fields.requiresPermit")}</dt>
                <dd>{plan.requires_permit === 1 ? t("labels.yes") : t("labels.no")}</dd>
                <dt className="text-text-muted">{t("fields.description")}</dt>
                <dd className="whitespace-pre-line">{plan.description || t("labels.none")}</dd>
              </dl>
              <div className="space-y-2 rounded-lg border border-dashed p-4">
                <h3 className="font-medium">{t("guidance.activationTitle")}</h3>
                <p className="text-sm text-text-muted">{t("guidance.activationPath")}</p>
                <p className="text-sm text-text-muted">{t("guidance.woCloseOutcome")}</p>
                <p className="text-sm">
                  {t("guidance.currentStatus", {
                    status: t(`lifecycle.${plan.lifecycle_status}`, {
                      defaultValue: plan.lifecycle_status,
                    }),
                  })}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <PermissionGate permission={P.PM_EDIT}>
                  <Button variant="outline" onClick={onEditPlan}>
                    {t("actions.editPlan")}
                  </Button>
                </PermissionGate>
                {lifecycleTargets.includes("proposed") ? (
                  <PermissionGate permission={P.PM_EDIT}>
                    <Button disabled={saving} onClick={() => onLifecycle("proposed")}>
                      {t("actions.propose")}
                    </Button>
                  </PermissionGate>
                ) : null}
                {lifecycleTargets.includes("approved") ? (
                  <PermissionGate permission={P.PM_APPROVE}>
                    <Button disabled={saving} onClick={() => onLifecycle("approved")}>
                      {t("actions.approve")}
                    </Button>
                  </PermissionGate>
                ) : null}
                {lifecycleTargets.includes("active") ? (
                  <PermissionGate permission={P.PM_EDIT}>
                    <Button disabled={saving} onClick={() => onLifecycle("active")}>
                      {t("actions.activate")}
                    </Button>
                  </PermissionGate>
                ) : null}
                {lifecycleTargets
                  .filter((s) => !["proposed", "approved", "active"].includes(s))
                  .map((s) => (
                    <PermissionGate key={s} permission={P.PM_EDIT}>
                      <Button variant="outline" disabled={saving} onClick={() => onLifecycle(s)}>
                        {t(`lifecycle.${s}`)}
                      </Button>
                    </PermissionGate>
                  ))}
                {plan.lifecycle_status === "draft" ? (
                  <PermissionGate permission={P.PM_DELETE}>
                    <Button variant="destructive" disabled={saving} onClick={onDeletePlan}>
                      {t("actions.deletePlan")}
                    </Button>
                  </PermissionGate>
                ) : null}
              </div>
            </TabsContent>

            <TabsContent value="versions" className="space-y-3">
              <PermissionGate permission={P.PM_CREATE}>
                <Button size="sm" onClick={onNewVersion}>
                  {t("actions.newVersion")}
                </Button>
              </PermissionGate>
              <DataTable columns={versionColumns} data={versions} />
              {versions.length === 0 && !detailLoading ? (
                <p className="text-sm text-text-muted">{t("empty.noVersions")}</p>
              ) : null}
            </TabsContent>

            <TabsContent value="occurrences" className="space-y-3">
              {focused ? (
                <section
                  className="space-y-2 rounded-lg border border-tone-info-border bg-tone-info-bg p-3"
                  aria-label={t("sections.focusedOccurrence")}
                >
                  <h4 className="text-sm font-medium">{t("sections.focusedOccurrence")}</h4>
                  <div className="flex flex-wrap items-center gap-3 text-sm">
                    <PmDueLabel occurrence={focused} />
                    <PmAssetCell code={focused.asset_code} name={focused.asset_name} />
                    <PmOccurrenceStatus occurrence={focused} />
                    <LinkedEntityBadge
                      entity="work_order"
                      code={focused.linked_work_order_code}
                      entityId={focused.linked_work_order_id}
                    />
                    <PermissionGate permission={P.PM_EDIT}>
                      {allowedOccurrenceTargets(focused.status).length > 0 ? (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => onTransitionOccurrence(focused)}
                        >
                          {t("actions.transitionOccurrence")}
                        </Button>
                      ) : null}
                      {canLogExecution(focused) ? (
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => onExecuteOccurrence(focused)}
                        >
                          {t("actions.logExecution")}
                        </Button>
                      ) : null}
                    </PermissionGate>
                  </div>
                </section>
              ) : null}
              <DataTable columns={occurrenceColumns} data={occurrences} />
              {occurrences.length === 0 && !detailLoading ? (
                <p className="text-sm text-text-muted">{t("empty.noOccurrences")}</p>
              ) : null}
            </TabsContent>

            <TabsContent value="executions" className="space-y-3">
              <DataTable columns={executionColumns} data={executions} />
              {executions.length === 0 && !detailLoading ? (
                <p className="text-sm text-text-muted">{t("empty.noExecutions")}</p>
              ) : null}
              {findingsFor != null ? (
                <section className="space-y-2 rounded-lg border p-3" aria-live="polite">
                  <h4 className="text-sm font-medium">{t("sections.findings")}</h4>
                  {findings.length === 0 ? (
                    <p className="text-sm text-text-muted">{t("empty.noFindings")}</p>
                  ) : (
                    <ul className="divide-y">
                      {findings.map((f) => (
                        <li key={f.id} className="space-y-1 py-2 text-sm">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-medium">{findingTypeLabel(f.finding_type)}</span>
                            {f.severity ? (
                              <StatusChip
                                label={t(`severity.${f.severity}`, { defaultValue: f.severity })}
                                tone={toneOf(PM_FINDING_SEVERITY_TONE, f.severity)}
                              />
                            ) : null}
                          </div>
                          <p className="whitespace-pre-line text-text-muted">{f.description}</p>
                          <PermissionGate permission={P.PM_CREATE}>
                            <Button
                              size="sm"
                              variant="outline"
                              disabled={
                                saving || revising === f.id || plan.lifecycle_status === "retired"
                              }
                              title={
                                plan.lifecycle_status === "retired"
                                  ? t("hints.reviseRetired")
                                  : t("hints.reviseFromFinding")
                              }
                              onClick={() => {
                                setRevising(f.id);
                                onReviseFromFinding(f)
                                  .then(() => setTab("versions"))
                                  .catch(() => {
                                    // Reported by the global IPC toast.
                                  })
                                  .finally(() => setRevising(null));
                              }}
                            >
                              {t("actions.revisePlan")}
                            </Button>
                          </PermissionGate>
                          {f.follow_up_di_code || f.follow_up_work_order_code ? (
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-xs text-text-muted">
                                {t("labels.followUps")}
                              </span>
                              <LinkedEntityBadge
                                entity="di"
                                code={f.follow_up_di_code}
                                entityId={f.follow_up_di_id}
                              />
                              <LinkedEntityBadge
                                entity="work_order"
                                code={f.follow_up_work_order_code}
                                entityId={f.follow_up_work_order_id}
                              />
                            </div>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  )}
                </section>
              ) : null}
              {recurringFindings.length > 0 ? (
                <section className="space-y-2 rounded-lg border p-3">
                  <h4 className="text-sm font-medium">
                    {t("sections.recurringFindings", { days: PM_RECURRING_FINDINGS_WINDOW_DAYS })}
                  </h4>
                  <ul className="space-y-1">
                    {recurringFindings.map((r) => (
                      <li
                        key={`${r.pm_plan_id}-${r.finding_type}`}
                        className="flex items-center justify-between gap-2 text-sm"
                      >
                        <span className="flex flex-wrap items-center gap-2">
                          {findingTypeLabel(r.finding_type)}
                          {r.latest_severity ? (
                            <StatusChip
                              label={t(`severity.${r.latest_severity}`, {
                                defaultValue: r.latest_severity,
                              })}
                              tone={toneOf(PM_FINDING_SEVERITY_TONE, r.latest_severity)}
                            />
                          ) : null}
                          <span className="text-xs text-text-muted">
                            {t("labels.lastSeenOn", { date: formatDate(r.last_seen_at) })}
                          </span>
                        </span>
                        <span className="tabular-nums text-text-muted">
                          {formatNumber(r.occurrence_count)}
                        </span>
                      </li>
                    ))}
                  </ul>
                </section>
              ) : null}
            </TabsContent>

            <TabsContent value="governance" className="space-y-4">
              <section className="space-y-2 rounded-lg border p-4">
                <h4 className="font-medium">{t("sections.readiness")}</h4>
                {!planningReadiness || planningReadiness.candidates.length === 0 ? (
                  <p className="text-sm text-text-muted">{t("empty.noReadiness")}</p>
                ) : (
                  <ul className="divide-y">
                    {planningReadiness.candidates.map((c) => (
                      <li
                        key={c.occurrence.id}
                        className="flex flex-wrap items-center justify-between gap-2 py-2 text-sm"
                      >
                        <span className="flex items-center gap-2">
                          <PmDueLabel occurrence={c.occurrence} />
                        </span>
                        <span className="flex flex-wrap gap-1">
                          {c.ready_for_scheduling ? (
                            <StatusChip label={t("labels.ready")} tone="success" />
                          ) : (
                            Array.from(new Set(c.blockers.map((b) => b.code))).map((code) => (
                              <StatusChip
                                key={code}
                                label={
                                  KNOWN_BLOCKERS.has(code)
                                    ? t(`blocker.${code}`)
                                    : t("labels.blockers", { count: 1 })
                                }
                                tone="warning"
                              />
                            ))
                          )}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
              <section className="space-y-2 rounded-lg border p-4">
                <h4 className="font-medium">{t("sections.kpis")}</h4>
                {!governanceKpis ? (
                  <p className="text-sm text-text-muted">{t("empty.noKpis")}</p>
                ) : (
                  <>
                    <p className="text-xs text-text-muted">
                      {t("kpi.period", {
                        from: formatDate(governanceKpis.from),
                        to: formatDate(governanceKpis.to),
                      })}
                    </p>
                    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                      <KpiTile
                        label={t("kpi.compliance")}
                        value={pct(governanceKpis.compliance)}
                        hint={ratio(governanceKpis.compliance)}
                      />
                      <KpiTile
                        label={t("kpi.overdueRisk")}
                        value={pct(governanceKpis.overdue_risk)}
                        hint={ratio(governanceKpis.overdue_risk)}
                      />
                      <KpiTile
                        label={t("kpi.firstPass")}
                        value={pct(governanceKpis.first_pass_completion)}
                        hint={ratio(governanceKpis.first_pass_completion)}
                      />
                      <KpiTile
                        label={t("kpi.followUpRatio")}
                        value={pct(governanceKpis.follow_up_ratio)}
                        hint={ratio(governanceKpis.follow_up_ratio)}
                      />
                      <KpiTile
                        label={t("kpi.failuresFoundByPm")}
                        value={formatNumber(governanceKpis.failures_found_by_pm)}
                        hint={t("kpi.failuresFoundByPmHint")}
                      />
                      <KpiTile
                        label={t("kpi.effortVariance")}
                        value={
                          governanceKpis.effort_variance.variance_pct != null
                            ? t("kpi.effortVarianceValue", {
                                hours: formatNumber(governanceKpis.effort_variance.variance_hours),
                                pct: formatPercent(governanceKpis.effort_variance.variance_pct),
                              })
                            : t("kpi.effortVarianceHours", {
                                hours: formatNumber(governanceKpis.effort_variance.variance_hours),
                              })
                        }
                        hint={t("kpi.sampleSize", {
                          count: governanceKpis.effort_variance.sample_size,
                        })}
                      />
                    </div>
                  </>
                )}
              </section>
            </TabsContent>
          </Tabs>
        </div>

        <DialogFooter className="border-t px-6 py-3">
          <Button variant="outline" onClick={onClose}>
            {t("actions.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

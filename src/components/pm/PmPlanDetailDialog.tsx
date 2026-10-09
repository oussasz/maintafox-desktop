import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatDate } from "@/utils/format-date";
import type {
  PmExecution,
  PmFinding,
  PmGovernanceKpiReport,
  PmOccurrence,
  PmPlan,
  PmPlanVersion,
  PmPlanningReadinessProjection,
  PmRecurringFinding,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { PUBLISHABLE_PLAN_LIFECYCLES, allowedPlanLifecycleTargets } from "./pm-labels";

interface PmPlanDetailDialogProps {
  open: boolean;
  plan: PmPlan | null;
  versions: PmPlanVersion[];
  occurrences: PmOccurrence[];
  executions: PmExecution[];
  findingsByExecutionId: Record<number, PmFinding[]>;
  recurringFindings: PmRecurringFinding[];
  planningReadiness: PmPlanningReadinessProjection | null;
  governanceKpis: PmGovernanceKpiReport | null;
  saving?: boolean;
  onClose: () => void;
  onEditPlan: () => void;
  onDeletePlan: () => void;
  onLifecycle: (next: string) => void;
  onNewVersion: () => void;
  onEditVersion: (version: PmPlanVersion) => void;
  onPublishVersion: (version: PmPlanVersion) => void;
  onDeleteVersion: (version: PmPlanVersion) => void;
  onTransitionOccurrence: (occurrence: PmOccurrence) => void;
  onExecuteOccurrence: (occurrence: PmOccurrence) => void;
  onLoadFindings: (executionId: number) => void;
}

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
}: PmPlanDetailDialogProps) {
  const { t, i18n } = useTranslation("pm");
  const [tab, setTab] = useState("overview");
  const [findingsFor, setFindingsFor] = useState<number | null>(null);

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
        cell: ({ row }) => <Badge variant="secondary">{row.original.status}</Badge>,
      },
      {
        id: "window",
        header: t("table.window"),
        cell: ({ row }) =>
          `${formatDate(row.original.effective_from, i18n.language)}${
            row.original.effective_to
              ? ` → ${formatDate(row.original.effective_to, i18n.language)}`
              : ""
          }`,
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => {
          const v = row.original;
          const isDraft = v.status === "draft";
          return (
            <div className="flex flex-wrap gap-1">
              <PermissionGate anyOf={[P.PM_EDIT, P.PM_CREATE]}>
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
              <PermissionGate anyOf={[P.PM_EDIT, P.PM_CREATE]}>
                <Button
                  size="sm"
                  disabled={
                    !isDraft ||
                    saving ||
                    !plan ||
                    !PUBLISHABLE_PLAN_LIFECYCLES.has(plan.lifecycle_status)
                  }
                  title={
                    !isDraft
                      ? t("hints.publishDraftOnly")
                      : plan && !PUBLISHABLE_PLAN_LIFECYCLES.has(plan.lifecycle_status)
                        ? t("hints.publishBlockedByLifecycle")
                        : undefined
                  }
                  onClick={() => onPublishVersion(v)}
                >
                  {t("actions.publish")}
                </Button>
              </PermissionGate>
              <PermissionGate anyOf={[P.PM_DELETE, P.PM_EDIT]}>
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={!isDraft || saving}
                  onClick={() => onDeleteVersion(v)}
                >
                  {t("actions.delete")}
                </Button>
              </PermissionGate>
            </div>
          );
        },
      },
    ],
    [t, i18n.language, saving, plan, onEditVersion, onPublishVersion, onDeleteVersion],
  );

  const occurrenceColumns: ColumnDef<PmOccurrence>[] = useMemo(
    () => [
      {
        id: "due",
        header: t("table.due"),
        cell: ({ row }) =>
          row.original.due_at
            ? formatDate(row.original.due_at, i18n.language)
            : row.original.due_meter_value != null
              ? String(row.original.due_meter_value)
              : t("labels.none"),
      },
      {
        accessorKey: "status",
        header: t("table.status"),
        cell: ({ row }) => (
          <Badge variant="secondary">
            {t(`occurrenceStatus.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        id: "wo",
        header: t("table.workOrder"),
        cell: ({ row }) => row.original.linked_work_order_code || t("labels.none"),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => (
          <div className="flex gap-1">
            <Button
              size="sm"
              variant="outline"
              onClick={() => onTransitionOccurrence(row.original)}
            >
              {t("actions.transitionOccurrence")}
            </Button>
            <Button size="sm" variant="secondary" onClick={() => onExecuteOccurrence(row.original)}>
              {t("actions.logExecution")}
            </Button>
          </div>
        ),
      },
    ],
    [t, i18n.language, onTransitionOccurrence, onExecuteOccurrence],
  );

  const executionColumns: ColumnDef<PmExecution>[] = useMemo(
    () => [
      {
        accessorKey: "executed_at",
        header: t("table.due"),
        cell: ({ row }) => formatDate(row.original.executed_at, i18n.language),
      },
      {
        accessorKey: "execution_result",
        header: t("table.result"),
        cell: ({ row }) =>
          t(`executionResult.${row.original.execution_result}`, {
            defaultValue: row.original.execution_result,
          }),
      },
      {
        id: "wo",
        header: t("table.workOrder"),
        cell: ({ row }) => row.original.work_order_code || t("labels.none"),
      },
      {
        id: "duration",
        header: t("table.actualDuration"),
        cell: ({ row }) =>
          row.original.actual_duration_hours != null
            ? String(row.original.actual_duration_hours)
            : t("labels.none"),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => (
          <Button
            size="sm"
            variant="outline"
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
    [t, i18n.language, onLoadFindings],
  );

  if (!plan) return null;

  const findings = findingsFor != null ? (findingsByExecutionId[findingsFor] ?? []) : [];

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent
        className="max-w-5xl max-h-[90vh] flex flex-col p-0 gap-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className="border-b px-6 py-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <DialogTitle className="flex flex-wrap items-center gap-2">
                <span className="font-mono text-base">{plan.code}</span>
                <span className="font-normal">{plan.title}</span>
              </DialogTitle>
              <div className="mt-2 flex flex-wrap gap-2">
                <Badge>
                  {t(`lifecycle.${plan.lifecycle_status}`, {
                    defaultValue: plan.lifecycle_status,
                  })}
                </Badge>
                <Badge variant="secondary">
                  {t(`strategy.${plan.strategy_type}`, { defaultValue: plan.strategy_type })}
                </Badge>
                {plan.assigned_team_label || plan.assigned_team_name ? (
                  <Badge variant="outline">
                    {plan.assigned_team_label || plan.assigned_team_name}
                  </Badge>
                ) : null}
              </div>
            </div>
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <Tabs value={tab} onValueChange={setTab}>
            <TabsList className="mb-4 flex flex-wrap h-auto">
              <TabsTrigger value="overview">{t("sections.overview")}</TabsTrigger>
              <TabsTrigger value="versions">{t("sections.versions")}</TabsTrigger>
              <TabsTrigger value="occurrences">{t("sections.occurrences")}</TabsTrigger>
              <TabsTrigger value="executions">{t("sections.executions")}</TabsTrigger>
              <TabsTrigger value="governance">{t("sections.governance")}</TabsTrigger>
            </TabsList>

            <TabsContent value="overview" className="space-y-4">
              <div className="rounded-lg border p-4 space-y-2 text-sm">
                <div>
                  <span className="text-muted-foreground">{t("fields.scopeType")}: </span>
                  {t(`scope.${plan.asset_scope_type}`, { defaultValue: plan.asset_scope_type })}
                </div>
                <div>
                  <span className="text-muted-foreground">{t("fields.criticality")}: </span>
                  {plan.criticality_label || t("labels.none")}
                </div>
                <div>
                  <span className="text-muted-foreground">{t("fields.requiresShutdown")}: </span>
                  {plan.requires_shutdown === 1 ? t("labels.yes") : t("labels.no")}
                </div>
                <div>
                  <span className="text-muted-foreground">{t("fields.requiresPermit")}: </span>
                  {plan.requires_permit === 1 ? t("labels.yes") : t("labels.no")}
                </div>
                <div>
                  <span className="text-muted-foreground">{t("fields.description")}: </span>
                  {plan.description || t("labels.none")}
                </div>
              </div>
              <div className="rounded-lg border border-dashed p-4 space-y-2">
                <h3 className="font-medium">{t("guidance.activationTitle")}</h3>
                <p className="text-sm text-muted-foreground">{t("guidance.activationPath")}</p>
                <p className="text-sm">
                  {t("guidance.currentStatus", {
                    status: t(`lifecycle.${plan.lifecycle_status}`, {
                      defaultValue: plan.lifecycle_status,
                    }),
                  })}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <PermissionGate anyOf={[P.PM_EDIT, P.PM_CREATE]}>
                  <Button variant="outline" onClick={onEditPlan}>
                    {t("actions.editPlan")}
                  </Button>
                </PermissionGate>
                {lifecycleTargets.includes("proposed") ? (
                  <PermissionGate anyOf={[P.PM_EDIT, P.PM_CREATE]}>
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
                  <PermissionGate anyOf={[P.PM_EDIT, P.PM_CREATE]}>
                    <Button disabled={saving} onClick={() => onLifecycle("active")}>
                      {t("actions.activate")}
                    </Button>
                  </PermissionGate>
                ) : null}
                {lifecycleTargets
                  .filter((s) => !["proposed", "approved", "active"].includes(s))
                  .map((s) => (
                    <PermissionGate key={s} anyOf={[P.PM_EDIT, P.PM_CREATE]}>
                      <Button variant="outline" disabled={saving} onClick={() => onLifecycle(s)}>
                        {t(`lifecycle.${s}`)}
                      </Button>
                    </PermissionGate>
                  ))}
                {plan.lifecycle_status === "draft" ? (
                  <PermissionGate anyOf={[P.PM_DELETE, P.PM_EDIT]}>
                    <Button variant="destructive" disabled={saving} onClick={onDeletePlan}>
                      {t("actions.deletePlan")}
                    </Button>
                  </PermissionGate>
                ) : null}
              </div>
            </TabsContent>

            <TabsContent value="versions" className="space-y-3">
              <PermissionGate anyOf={[P.PM_CREATE, P.PM_EDIT]}>
                <Button size="sm" onClick={onNewVersion}>
                  {t("actions.newVersion")}
                </Button>
              </PermissionGate>
              <DataTable columns={versionColumns} data={versions} />
              {versions.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("empty.noVersions")}</p>
              ) : null}
            </TabsContent>

            <TabsContent value="occurrences" className="space-y-3">
              <DataTable columns={occurrenceColumns} data={occurrences} />
              {occurrences.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("empty.noOccurrences")}</p>
              ) : null}
            </TabsContent>

            <TabsContent value="executions" className="space-y-3">
              <DataTable columns={executionColumns} data={executions} />
              {executions.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("empty.noExecutions")}</p>
              ) : null}
              {findingsFor != null ? (
                <div className="rounded-lg border p-3 space-y-2">
                  <h4 className="text-sm font-medium">{t("sections.findings")}</h4>
                  {findings.length === 0 ? (
                    <p className="text-sm text-muted-foreground">{t("labels.none")}</p>
                  ) : (
                    findings.map((f) => (
                      <div key={f.id} className="text-sm border-b last:border-0 py-2">
                        <div className="font-medium">
                          {f.finding_type}
                          {f.severity ? ` · ${f.severity}` : ""}
                        </div>
                        <div className="text-muted-foreground">{f.description}</div>
                      </div>
                    ))
                  )}
                </div>
              ) : null}
              {recurringFindings.length > 0 ? (
                <div className="rounded-lg border p-3 space-y-2">
                  <h4 className="text-sm font-medium">{t("sections.recurringFindings")}</h4>
                  {recurringFindings.map((r, idx) => (
                    <div
                      key={`${r.finding_type}-${idx}`}
                      className="text-sm flex justify-between gap-2"
                    >
                      <span>
                        {r.finding_type}
                        {r.latest_severity ? ` · ${r.latest_severity}` : ""}
                      </span>
                      <span className="tabular-nums text-muted-foreground">
                        {r.occurrence_count}
                      </span>
                    </div>
                  ))}
                </div>
              ) : null}
            </TabsContent>

            <TabsContent value="governance" className="space-y-4">
              <div className="rounded-lg border p-4 space-y-2">
                <h4 className="font-medium">{t("sections.readiness")}</h4>
                {!planningReadiness || planningReadiness.candidates.length === 0 ? (
                  <p className="text-sm text-muted-foreground">{t("empty.noReadiness")}</p>
                ) : (
                  planningReadiness.candidates.slice(0, 20).map((c) => (
                    <div
                      key={c.occurrence.id}
                      className="text-sm flex flex-wrap items-center justify-between gap-2 border-b py-2 last:border-0"
                    >
                      <span className="font-mono text-xs">{c.occurrence.plan_code}</span>
                      <Badge variant={c.ready_for_scheduling ? "default" : "secondary"}>
                        {c.ready_for_scheduling
                          ? t("occurrenceStatus.ready_for_scheduling")
                          : t("labels.blockers", { count: c.blockers.length })}
                      </Badge>
                    </div>
                  ))
                )}
              </div>
              <div className="rounded-lg border p-4 space-y-2">
                <h4 className="font-medium">{t("sections.kpis")}</h4>
                {!governanceKpis ? (
                  <p className="text-sm text-muted-foreground">{t("empty.noKpis")}</p>
                ) : (
                  <div className="grid gap-2 text-sm md:grid-cols-2">
                    <div>
                      Compliance:{" "}
                      {governanceKpis.compliance.value_pct != null
                        ? `${governanceKpis.compliance.value_pct.toFixed(1)}%`
                        : t("labels.none")}
                    </div>
                    <div>
                      Overdue risk:{" "}
                      {governanceKpis.overdue_risk.value_pct != null
                        ? `${governanceKpis.overdue_risk.value_pct.toFixed(1)}%`
                        : t("labels.none")}
                    </div>
                    <div>
                      First-pass:{" "}
                      {governanceKpis.first_pass_completion.value_pct != null
                        ? `${governanceKpis.first_pass_completion.value_pct.toFixed(1)}%`
                        : t("labels.none")}
                    </div>
                    <div>
                      Follow-up ratio:{" "}
                      {governanceKpis.follow_up_ratio.value_pct != null
                        ? `${governanceKpis.follow_up_ratio.value_pct.toFixed(1)}%`
                        : t("labels.none")}
                    </div>
                    <div className="md:col-span-2 text-muted-foreground">
                      Effort variance: {governanceKpis.effort_variance.variance_hours.toFixed(1)} h
                      {governanceKpis.effort_variance.variance_pct != null
                        ? ` (${governanceKpis.effort_variance.variance_pct.toFixed(1)}%)`
                        : ""}
                      {" · "}
                      n={governanceKpis.effort_variance.sample_size}
                    </div>
                  </div>
                )}
              </div>
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

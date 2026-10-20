/**
 * Right-pane personnel detail workspace (directory tab master/detail).
 * Unified EntityDetail* chrome — same sections as the former modal dialog.
 */

import { Building2, MoreHorizontal, Pencil, UserX, Users, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  DetailFieldRow,
  DetailSectionCard,
  ENTITY_DETAIL_HEADER_ACTION_BTN,
  EntityDetailHeader,
  EntityDetailViewSwitcher,
} from "@/components/detail";
import { PersonScheduleExceptions } from "@/components/personnel/PersonScheduleExceptions";
import { PersonnelPortrait } from "@/components/personnel/PersonnelPortrait";
import { PersonCapacityDays } from "@/components/personnel/TeamCapacityDays";
import { absenceBlockTitle } from "@/components/personnel/absence-block-display";
import { PersonQualificationSheet } from "@/components/personnel/qualification/PersonQualificationSheet";
import { usePersonnelLists } from "@/components/personnel/use-personnel-lists";
import { Timeline, type TimelineEntry } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ReviewMark } from "@/components/ui/review-mark";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { useWoOpenRuleText } from "@/lib/wo-open-rule";
import {
  createRateCard,
  getPersonnelWorkloadSummary,
  listPersonnelAssignmentHistory,
  listPersonnelAvailabilityBlocks,
  listPersonnelTeamAssignments,
  listPersonnelWorkHistory,
} from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type {
  PersonnelAssignmentHistoryEntry,
  PersonnelAvailabilityBlock,
  PersonnelDetailPayload,
  PersonnelRateCard,
  PersonnelTeamAssignment,
  PersonnelWorkHistoryEntry,
  PersonnelWorkloadSummary,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type PersonnelDetailView =
  | "identity"
  | "skills"
  | "availability"
  | "teams"
  | "rates"
  | "authorizations"
  | "assignment"
  | "history"
  | "workload";

export interface PersonnelDetailWorkspaceProps {
  detail: PersonnelDetailPayload;
}

function fmt(iso: string | null | undefined): string {
  if (!iso) return "—";
  return iso.slice(0, 10);
}

function toAssignmentEntry(h: PersonnelAssignmentHistoryEntry): TimelineEntry {
  const titleParts: string[] = [];
  if (h.entity_name) titleParts.push(h.entity_name);
  if (h.team_name) titleParts.push(h.team_name);
  if (h.position_code || h.position_name) {
    titleParts.push([h.position_code, h.position_name].filter(Boolean).join(" — "));
  }

  const badges = h.manager_name ? [{ label: h.manager_name, tone: "muted" as const }] : undefined;

  return {
    id: String(h.id),
    timestamp: h.started_at,
    title: titleParts.join(" · ") || "—",
    subtitle: h.schedule_name ?? undefined,
    description: h.reason ?? (h.ended_at ? `Until ${fmt(h.ended_at)}` : "Current"),
    badges,
    icon: h.team_id ? <Users className="h-3.5 w-3.5" /> : <Building2 className="h-3.5 w-3.5" />,
  };
}

function toWorkHistoryEntry(h: PersonnelWorkHistoryEntry): TimelineEntry {
  return {
    id: `${h.source_module}-${h.record_id}-${h.role_code}`,
    timestamp: h.happened_at,
    title: `${h.source_module.toUpperCase()} ${h.record_code ?? h.record_id}`,
    subtitle: `${h.role_code} · ${h.status_code ?? "—"}`,
  };
}

function EmptyMsg({ msg }: { msg: string }) {
  return <p className="text-sm text-text-muted">{msg}</p>;
}

function pickActiveRateCard(cards: PersonnelRateCard[]): PersonnelRateCard | null {
  const today = new Date().toISOString().slice(0, 10);
  const eligible = cards
    .filter((c) => String(c.effective_from).slice(0, 10) <= today)
    .sort((a, b) => String(b.effective_from).localeCompare(String(a.effective_from)));
  return eligible[0] ?? null;
}

/** Data read for the detail tabs (each may fail on its own). */
type TabDataKey = "teams" | "availability" | "history" | "assignment" | "workload";

export function PersonnelDetailWorkspace({ detail }: PersonnelDetailWorkspaceProps) {
  const { t } = useTranslation("personnel");
  // Employment statuses and absence types come from the plant's lists (Données de référence).
  const lists = usePersonnelLists();
  const woOpenRuleText = useWoOpenRuleText();
  const { formatCurrency } = useFormatters();
  const { can } = usePermissions();
  const canManage = can(P.PER_MANAGE);
  const closePersonnel = usePersonnelStore((s) => s.closePersonnel);
  const openCreateForm = usePersonnelStore((s) => s.openCreateForm);
  const openPersonnel = usePersonnelStore((s) => s.openPersonnel);
  const deactivate = usePersonnelStore((s) => s.deactivate);

  const p = detail.personnel;
  const statusValue = lists.option("employment_statuses", p.employment_status);
  const statusToReview =
    !!p.employment_status &&
    lists.canRead &&
    !lists.loading &&
    (statusValue == null || statusValue.review_required);
  /** Server withheld per.sensitiveview fields; show them as restricted, never as empty. */
  const restricted = p.sensitive_masked === true;
  const sensitive = (value: string | null | undefined): string | null =>
    restricted ? t("detail.restricted") : (value ?? null);
  const [activeView, setActiveView] = useState<PersonnelDetailView>("identity");
  const [teamAssignments, setTeamAssignments] = useState<PersonnelTeamAssignment[]>([]);
  const [blocks, setBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [workHistory, setWorkHistory] = useState<PersonnelWorkHistoryEntry[]>([]);
  const [assignmentHistory, setAssignmentHistory] = useState<PersonnelAssignmentHistoryEntry[]>([]);
  const [workload, setWorkload] = useState<PersonnelWorkloadSummary | null>(null);
  const [tabsLoading, setTabsLoading] = useState(false);
  /** Per tab: why its data could not be read (never shown as "no data" / "available"). */
  const [tabErrors, setTabErrors] = useState<Partial<Record<TabDataKey, unknown>>>({});
  const [tabsReload, setTabsReload] = useState(0);
  const [laborRateInput, setLaborRateInput] = useState("");
  const [overtimeRateInput, setOvertimeRateInput] = useState("");
  const [rateSaving, setRateSaving] = useState(false);
  const [rateError, setRateError] = useState<string | null>(null);

  const rateCards = useMemo(() => detail.rate_cards ?? [], [detail.rate_cards]);
  const activeRate = useMemo(() => pickActiveRateCard(rateCards), [rateCards]);

  const submitRateCard = useCallback(async () => {
    setRateError(null);
    const laborTrim = laborRateInput.trim();
    const overtimeTrim = overtimeRateInput.trim();
    if (!laborTrim) {
      setRateError(t("validation.laborRateInvalid"));
      return;
    }
    const laborNum = Number(laborTrim.replace(",", "."));
    if (!Number.isFinite(laborNum) || laborNum < 0) {
      setRateError(t("validation.laborRateInvalid"));
      return;
    }
    let overtimeNum = 0;
    if (overtimeTrim) {
      overtimeNum = Number(overtimeTrim.replace(",", "."));
      if (!Number.isFinite(overtimeNum) || overtimeNum < 0) {
        setRateError(t("validation.overtimeRateInvalid"));
        return;
      }
    }
    setRateSaving(true);
    try {
      await createRateCard(p.id, laborNum, overtimeNum, null, "manual");
      setLaborRateInput("");
      setOvertimeRateInput("");
      await openPersonnel(p.id);
    } catch (err) {
      setRateError(describeAppError(err).title);
    } finally {
      setRateSaving(false);
    }
  }, [laborRateInput, overtimeRateInput, p.id, openPersonnel, t]);

  useEffect(() => {
    setActiveView("identity");
  }, [p.id]);

  useEffect(() => {
    let cancelled = false;
    setTabsLoading(true);
    setTabErrors({});
    void Promise.allSettled([
      listPersonnelTeamAssignments(p.id),
      listPersonnelAvailabilityBlocks(p.id, 20),
      listPersonnelWorkHistory(p.id, 40),
      listPersonnelAssignmentHistory(p.id, 60),
      getPersonnelWorkloadSummary(p.id),
    ]).then(([teams, bl, wh, ah, wl]) => {
      if (cancelled) return;
      const errors: Partial<Record<TabDataKey, unknown>> = {};
      if (teams.status === "fulfilled") setTeamAssignments(teams.value);
      else errors.teams = teams.reason;
      if (bl.status === "fulfilled") setBlocks(bl.value);
      else {
        setBlocks([]);
        errors.availability = bl.reason;
      }
      if (wh.status === "fulfilled") setWorkHistory(wh.value);
      else errors.history = wh.reason;
      if (ah.status === "fulfilled") setAssignmentHistory(ah.value);
      else errors.assignment = ah.reason;
      if (wl.status === "fulfilled") setWorkload(wl.value);
      else errors.workload = wl.reason;
      setTabErrors(errors);
      setTabsLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [p.id, tabsReload]);

  const tabError = (key: TabDataKey) =>
    tabErrors[key] != null ? (
      <ErrorState
        error={tabErrors[key]}
        onRetry={() => setTabsReload((n) => n + 1)}
        retrying={tabsLoading}
        className="py-4"
      />
    ) : null;

  const viewTabs = useMemo(
    () => [
      { id: "identity", label: t("detail.tabs.identity") },
      { id: "skills", label: t("detail.tabs.qualifications") },
      { id: "availability", label: t("detail.tabs.availability") },
      { id: "teams", label: t("detail.tabs.teams") },
      { id: "rates", label: t("detail.tabs.rates") },
      { id: "authorizations", label: t("detail.tabs.authorizations") },
      { id: "assignment", label: t("detail.tabs.assignment") },
      { id: "history", label: t("detail.tabs.history") },
      { id: "workload", label: t("detail.tabs.workload") },
    ],
    [t],
  );

  const assignmentEntries = useMemo<TimelineEntry[]>(
    () => assignmentHistory.map(toAssignmentEntry),
    [assignmentHistory],
  );

  const workEntries = useMemo<TimelineEntry[]>(
    () => workHistory.map(toWorkHistoryEntry),
    [workHistory],
  );

  const isExternal = p.employment_origin === "external";
  const isContractor = p.employment_type === "contractor" || p.employment_type === "vendor";
  const isActive = p.employment_status !== "terminated" && p.employment_status !== "inactive";

  const positionValue = [p.position_code, p.position_name].filter(Boolean).join(" — ") || "—";

  return (
    <div className="flex h-full flex-col space-y-4 overflow-auto p-4">
      <EntityDetailHeader
        code={p.employee_code}
        designation={p.full_name}
        statusSlot={
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge variant="outline" className="text-[10px] capitalize">
              {p.employment_origin
                ? t(`employmentOrigin.${p.employment_origin}`, p.employment_origin)
                : "—"}
            </Badge>
            <Badge variant="outline" className="text-[10px]">
              {p.employment_status ? lists.text("employment_statuses", p.employment_status) : "—"}
            </Badge>
            {statusToReview ? <ReviewMark /> : null}
          </div>
        }
        qrSlot={
          <PersonnelPortrait personnelId={p.id} photoPath={p.photo_path} fullName={p.full_name} />
        }
        primaryActionsSlot={
          <>
            <PermissionGate permission={P.PER_MANAGE}>
              <Button
                variant="default"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={() => openCreateForm(p)}
              >
                <Pencil className="h-3.5 w-3.5" />
                {t("detail.actions.edit")}
              </Button>
            </PermissionGate>
            <Button
              variant="outline"
              size="sm"
              className={ENTITY_DETAIL_HEADER_ACTION_BTN}
              onClick={() => closePersonnel()}
              aria-label={t("detail.actions.close")}
            >
              <X className="h-3.5 w-3.5" />
              {t("detail.actions.close")}
            </Button>
          </>
        }
        overflowSlot={
          isActive ? (
            <PermissionGate permission={P.PER_MANAGE}>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 w-7 px-0"
                    aria-label={t("detail.actions.more")}
                  >
                    <MoreHorizontal className="h-3.5 w-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-48">
                  <DropdownMenuItem className="gap-2" onClick={() => openCreateForm(p)}>
                    <Pencil className="h-3.5 w-3.5" />
                    {t("detail.actions.edit")}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    className="gap-2 text-status-danger focus:text-status-danger"
                    onClick={() => void deactivate(p.id, p.row_version)}
                  >
                    <UserX className="h-3.5 w-3.5" />
                    {t("detail.actions.deactivate")}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </PermissionGate>
          ) : null
        }
      />

      {isContractor && isExternal ? (
        <div className="rounded border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg">
          {t("detail.contractorBanner", {
            company: p.company_name ?? "—",
          })}
        </div>
      ) : null}

      <EntityDetailViewSwitcher
        tabs={viewTabs}
        activeId={activeView}
        onChange={(id) => setActiveView(id as PersonnelDetailView)}
      />

      {activeView === "identity" ? (
        <DetailSectionCard title={t("detail.tabs.identity")}>
          <DetailFieldRow label={t("field.position")} value={positionValue} />
          <DetailFieldRow label={t("field.entity")} value={p.entity_name} />
          <DetailFieldRow label={t("field.team")} value={p.team_name} />
          <DetailFieldRow label={t("field.schedule")} value={p.schedule_name} />
          <DetailFieldRow label={t("field.supervisor")} value={p.supervisor_name} />
          <DetailFieldRow label={t("field.employmentOrigin")} value={p.employment_origin} />
          <DetailFieldRow
            label={t("field.employmentStatus")}
            value={
              p.employment_status ? lists.text("employment_statuses", p.employment_status) : null
            }
          />
          <DetailFieldRow label={t("field.hireDate")} value={fmt(p.hire_date)} />
          {isExternal ? (
            <>
              <DetailFieldRow label={t("field.company")} value={p.company_name} />
              <DetailFieldRow
                label={t("field.contractNumber")}
                value={sensitive(p.contract_number)}
              />
              <DetailFieldRow
                label={t("field.contractStart")}
                value={sensitive(fmt(p.contract_start_date))}
              />
              <DetailFieldRow
                label={t("field.contractEnd")}
                value={sensitive(fmt(p.contract_end_date))}
              />
            </>
          ) : null}
          <DetailFieldRow label={t("field.email")} value={sensitive(p.email)} />
          <DetailFieldRow label={t("field.phone")} value={sensitive(p.phone)} />
          <DetailFieldRow
            label={t("field.laborRate")}
            value={
              restricted
                ? t("detail.restricted")
                : activeRate
                  ? formatCurrency(activeRate.labor_rate)
                  : t("detail.rates.overviewMissing")
            }
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "skills" ? (
        <DetailSectionCard title={t("detail.tabs.qualifications")}>
          <PersonQualificationSheet personnelId={p.id} />
        </DetailSectionCard>
      ) : null}

      {activeView === "availability" ? (
        <DetailSectionCard title={t("detail.tabs.availability")}>
          <p className="text-sm text-text-muted">{t("capacityDays.personIntro")}</p>
          <PersonCapacityDays personnelId={p.id} />
          <PersonScheduleExceptions personnelId={p.id} />
          {tabError("availability")}
          {blocks.length > 0 ? (
            <h4 className="pt-2 text-sm font-semibold text-text-primary">
              {t("availability.detail.blocks")}
            </h4>
          ) : null}
          {blocks.map((b) => (
            <div key={b.id} className="rounded border border-surface-border p-2 text-sm">
              <span className="font-medium">{absenceBlockTitle(b, lists, t)}</span> ·{" "}
              {fmt(b.start_at)} → {fmt(b.end_at)}
            </div>
          ))}
        </DetailSectionCard>
      ) : null}

      {activeView === "teams" ? (
        <DetailSectionCard title={t("detail.tabs.teams")}>
          {tabError("teams")}
          {!tabsLoading && tabErrors.teams == null && teamAssignments.length === 0 ? (
            <EmptyMsg msg={t("common.noData")} />
          ) : null}
          {teamAssignments.map((ta) => (
            <div key={ta.id} className="rounded border border-surface-border p-2">
              <div className="font-medium">{ta.team_name ?? "—"}</div>
              <div className="text-text-muted">
                {ta.role_code} · {Math.round(ta.allocation_percent)}%
              </div>
            </div>
          ))}
        </DetailSectionCard>
      ) : null}

      {activeView === "rates" ? (
        <DetailSectionCard title={t("detail.tabs.rates")}>
          <p className="mb-2 text-[11px] text-text-muted">{t("detail.rates.hint")}</p>
          {restricted ? <EmptyMsg msg={t("detail.rates.restricted")} /> : null}
          {!restricted && rateCards.length === 0 ? (
            <EmptyMsg msg={t("detail.rates.empty")} />
          ) : null}
          <div className="space-y-2">
            {rateCards.map((rc) => {
              const isActive = activeRate?.id === rc.id;
              return (
                <div key={rc.id} className="rounded border border-surface-border p-2 text-sm">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">
                      {t("detail.rates.effectiveFrom")}: {rc.effective_from}
                    </span>
                    {isActive ? (
                      <Badge variant="secondary" className="text-[10px]">
                        {t("detail.rates.active")}
                      </Badge>
                    ) : null}
                  </div>
                  <div className="mt-1 text-text-muted">
                    {t("detail.rates.labor")}: {formatCurrency(rc.labor_rate)} ·{" "}
                    {t("detail.rates.overtime")}: {formatCurrency(rc.overtime_rate)}
                  </div>
                  <div className="text-[11px] text-text-muted">
                    {t("detail.rates.source")}: {rc.source_type}
                  </div>
                </div>
              );
            })}
          </div>
          {canManage ? (
            <div className="mt-3 space-y-2 rounded border border-dashed border-surface-border p-3">
              <div className="text-sm font-medium">{t("detail.rates.addTitle")}</div>
              {rateError ? <p className="text-sm text-destructive">{rateError}</p> : null}
              <div className="grid gap-2 sm:grid-cols-2">
                <div className="space-y-1">
                  <Label htmlFor="detail-labor-rate">{t("field.laborRate")}</Label>
                  <Input
                    id="detail-labor-rate"
                    type="number"
                    min={0}
                    step="0.01"
                    inputMode="decimal"
                    value={laborRateInput}
                    onChange={(e) => setLaborRateInput(e.target.value)}
                    placeholder={t("field.laborRatePlaceholder")}
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="detail-overtime-rate">{t("field.overtimeRate")}</Label>
                  <Input
                    id="detail-overtime-rate"
                    type="number"
                    min={0}
                    step="0.01"
                    inputMode="decimal"
                    value={overtimeRateInput}
                    onChange={(e) => setOvertimeRateInput(e.target.value)}
                    placeholder={t("field.overtimeRatePlaceholder")}
                  />
                </div>
              </div>
              <Button
                type="button"
                size="sm"
                disabled={rateSaving}
                onClick={() => void submitRateCard()}
              >
                {rateSaving ? t("detail.rates.saving") : t("detail.rates.add")}
              </Button>
            </div>
          ) : null}
        </DetailSectionCard>
      ) : null}

      {activeView === "authorizations" ? (
        <DetailSectionCard title={t("detail.tabs.authorizations")}>
          {(detail.authorizations ?? []).length === 0 ? (
            <EmptyMsg msg={t("common.noData")} />
          ) : null}
          {(detail.authorizations ?? []).map((a) => (
            <div key={a.id} className="rounded border border-surface-border p-2">
              {a.authorization_type} · {a.valid_from} → {a.valid_to ?? "—"}
            </div>
          ))}
        </DetailSectionCard>
      ) : null}

      {activeView === "assignment" ? (
        <DetailSectionCard title={t("detail.tabs.assignment")}>
          <Timeline
            items={assignmentEntries}
            loading={tabsLoading}
            error={tabErrors.assignment ?? undefined}
            onRetry={() => setTabsReload((n) => n + 1)}
            aria-label={t("detail.tabs.assignment")}
            empty={t("common.noData")}
            groupBy="day"
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "history" ? (
        <DetailSectionCard title={t("detail.tabs.history")}>
          <Timeline
            items={workEntries}
            loading={tabsLoading}
            error={tabErrors.history ?? undefined}
            onRetry={() => setTabsReload((n) => n + 1)}
            aria-label={t("detail.tabs.history")}
            empty={t("common.noData")}
            groupBy="day"
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "workload" ? (
        <DetailSectionCard title={t("detail.tabs.workload")}>
          {tabsLoading ? <EmptyMsg msg={t("common.loading")} /> : null}
          {tabError("workload")}
          {workload ? (
            <>
              <DetailFieldRow
                label={
                  woOpenRuleText(workload.open_work_orders_rule)?.figure ??
                  t("detail.workload.openWo")
                }
                value={String(workload.open_work_orders)}
              />
              <DetailFieldRow
                label={
                  woOpenRuleText(workload.in_progress_work_orders_rule)?.figure ??
                  t("detail.workload.inProgressWo")
                }
                value={String(workload.in_progress_work_orders)}
              />
              <DetailFieldRow
                label={t("detail.workload.pendingDi")}
                value={String(workload.pending_interventions)}
              />
              <DetailFieldRow
                label={t("detail.workload.di30d")}
                value={String(workload.interventions_last_30d)}
              />
            </>
          ) : null}
          {!tabsLoading && !workload && tabErrors.workload == null ? (
            <EmptyMsg msg={t("common.noData")} />
          ) : null}
        </DetailSectionCard>
      ) : null}
    </div>
  );
}

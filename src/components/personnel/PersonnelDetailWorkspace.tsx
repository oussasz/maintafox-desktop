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
import { PersonnelPortrait } from "@/components/personnel/PersonnelPortrait";
import { Timeline, type TimelineEntry } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  createRateCard,
  getPersonnelWorkloadSummary,
  listAvailabilityCalendar,
  listPersonnelAssignmentHistory,
  listPersonnelAvailabilityBlocks,
  listPersonnelTeamAssignments,
  listPersonnelWorkHistory,
  listSkillsMatrix,
} from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type {
  AvailabilityCalendarEntry,
  PersonnelAssignmentHistoryEntry,
  PersonnelAvailabilityBlock,
  PersonnelDetailPayload,
  PersonnelRateCard,
  PersonnelTeamAssignment,
  PersonnelWorkHistoryEntry,
  PersonnelWorkloadSummary,
  SkillMatrixRow,
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

function dateRange(days = 14) {
  const from = new Date();
  const to = new Date();
  to.setDate(to.getDate() + days);
  return { from: from.toISOString().slice(0, 10), to: to.toISOString().slice(0, 10) };
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

export function PersonnelDetailWorkspace({ detail }: PersonnelDetailWorkspaceProps) {
  const { t } = useTranslation("personnel");
  const { formatCurrency } = useFormatters();
  const { can } = usePermissions();
  const canManage = can(P.PER_MANAGE);
  const closePersonnel = usePersonnelStore((s) => s.closePersonnel);
  const openCreateForm = usePersonnelStore((s) => s.openCreateForm);
  const openPersonnel = usePersonnelStore((s) => s.openPersonnel);
  const deactivate = usePersonnelStore((s) => s.deactivate);

  const p = detail.personnel;
  const [activeView, setActiveView] = useState<PersonnelDetailView>("identity");
  const [skills, setSkills] = useState<SkillMatrixRow[]>([]);
  const [calendarRows, setCalendarRows] = useState<AvailabilityCalendarEntry[]>([]);
  const [teamAssignments, setTeamAssignments] = useState<PersonnelTeamAssignment[]>([]);
  const [blocks, setBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [workHistory, setWorkHistory] = useState<PersonnelWorkHistoryEntry[]>([]);
  const [assignmentHistory, setAssignmentHistory] = useState<PersonnelAssignmentHistoryEntry[]>([]);
  const [workload, setWorkload] = useState<PersonnelWorkloadSummary | null>(null);
  const [tabsLoading, setTabsLoading] = useState(false);
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
      setRateError(err instanceof Error ? err.message : String(err));
    } finally {
      setRateSaving(false);
    }
  }, [laborRateInput, overtimeRateInput, p.id, openPersonnel, t]);

  useEffect(() => {
    setActiveView("identity");
    const range = dateRange();
    setTabsLoading(true);
    void Promise.all([
      listSkillsMatrix({ personnel_id: p.id, include_inactive: true }),
      listAvailabilityCalendar({ date_from: range.from, date_to: range.to, personnel_id: p.id }),
      listPersonnelTeamAssignments(p.id),
      listPersonnelAvailabilityBlocks(p.id, 20),
      listPersonnelWorkHistory(p.id, 40),
      listPersonnelAssignmentHistory(p.id, 60),
      getPersonnelWorkloadSummary(p.id),
    ])
      .then(([s, cal, teams, bl, wh, ah, wl]) => {
        setSkills(s);
        setCalendarRows(cal);
        setTeamAssignments(teams);
        setBlocks(bl);
        setWorkHistory(wh);
        setAssignmentHistory(ah);
        setWorkload(wl);
      })
      .catch(() => {
        /* individual views handle empty state */
      })
      .finally(() => setTabsLoading(false));
  }, [p.id]);

  const viewTabs = useMemo(
    () => [
      { id: "identity", label: t("detail.tabs.identity") },
      { id: "skills", label: t("detail.tabs.skills") },
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

  const calendarByDate = useMemo(() => {
    const map = new Map<string, AvailabilityCalendarEntry>();
    for (const row of calendarRows) map.set(row.work_date, row);
    return [...map.values()];
  }, [calendarRows]);

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
            <Badge variant="outline" className="text-[10px] capitalize">
              {p.employment_status
                ? t(`employmentStatus.${p.employment_status}`, p.employment_status)
                : "—"}
            </Badge>
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
        <div className="rounded border border-amber-400/50 bg-amber-100/40 px-3 py-2 text-sm text-amber-900">
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
          <DetailFieldRow label={t("field.employmentStatus")} value={p.employment_status} />
          <DetailFieldRow label={t("field.hireDate")} value={fmt(p.hire_date)} />
          {isExternal ? (
            <>
              <DetailFieldRow label={t("field.company")} value={p.company_name} />
              <DetailFieldRow label={t("field.contractNumber")} value={p.contract_number} />
              <DetailFieldRow label={t("field.contractStart")} value={fmt(p.contract_start_date)} />
              <DetailFieldRow label={t("field.contractEnd")} value={fmt(p.contract_end_date)} />
            </>
          ) : null}
          <DetailFieldRow label={t("field.email")} value={p.email} />
          <DetailFieldRow label={t("field.phone")} value={p.phone} />
          <DetailFieldRow
            label={t("field.laborRate")}
            value={
              activeRate ? formatCurrency(activeRate.labor_rate) : t("detail.rates.overviewMissing")
            }
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "skills" ? (
        <DetailSectionCard title={t("detail.tabs.skills")}>
          {tabsLoading ? <EmptyMsg msg={t("common.loading")} /> : null}
          {!tabsLoading && skills.length === 0 ? <EmptyMsg msg={t("common.noData")} /> : null}
          {skills.map((row) => (
            <div
              key={`${row.personnel_id}-${row.skill_code ?? "x"}`}
              className="rounded border border-surface-border p-2"
            >
              <div className="font-medium">{row.skill_label ?? "—"}</div>
              <div className="text-text-muted">
                {t("skills.columns.level")}: {row.proficiency_level ?? "—"} ·{" "}
                {t(`skills.coverage.${row.coverage_status}`, row.coverage_status)}
              </div>
            </div>
          ))}
        </DetailSectionCard>
      ) : null}

      {activeView === "availability" ? (
        <DetailSectionCard title={t("detail.tabs.availability")}>
          {calendarByDate.length === 0 && !tabsLoading ? (
            <EmptyMsg msg={t("common.noData")} />
          ) : null}
          <div className="grid gap-2 md:grid-cols-2">
            {calendarByDate.map((row) => (
              <div key={row.work_date} className="rounded border border-surface-border p-2">
                <div className="font-medium">{row.work_date}</div>
                <div className="text-text-muted">
                  {row.available_minutes}m {t("detail.available")} / {row.blocked_minutes}m{" "}
                  {t("detail.blocked")}
                </div>
              </div>
            ))}
          </div>
          {blocks.map((b) => (
            <div key={b.id} className="rounded border border-surface-border p-2 text-sm">
              <span className="font-medium">{b.block_type}</span> · {fmt(b.start_at)} →{" "}
              {fmt(b.end_at)}
            </div>
          ))}
        </DetailSectionCard>
      ) : null}

      {activeView === "teams" ? (
        <DetailSectionCard title={t("detail.tabs.teams")}>
          {!tabsLoading && teamAssignments.length === 0 ? (
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
          {rateCards.length === 0 ? <EmptyMsg msg={t("detail.rates.empty")} /> : null}
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
            aria-label={t("detail.tabs.assignment")}
            empty={t("common.noData")}
            groupBy="day"
            todayLabel={t("common.today", "Today")}
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "history" ? (
        <DetailSectionCard title={t("detail.tabs.history")}>
          <Timeline
            items={workEntries}
            loading={tabsLoading}
            aria-label={t("detail.tabs.history")}
            empty={t("common.noData")}
            groupBy="day"
            todayLabel={t("common.today", "Today")}
          />
        </DetailSectionCard>
      ) : null}

      {activeView === "workload" ? (
        <DetailSectionCard title={t("detail.tabs.workload")}>
          {tabsLoading ? <EmptyMsg msg={t("common.loading")} /> : null}
          {workload ? (
            <>
              <DetailFieldRow
                label={t("detail.workload.openWo")}
                value={String(workload.open_work_orders)}
              />
              <DetailFieldRow
                label={t("detail.workload.inProgressWo")}
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
          {!tabsLoading && !workload ? <EmptyMsg msg={t("common.noData")} /> : null}
        </DetailSectionCard>
      ) : null}
    </div>
  );
}

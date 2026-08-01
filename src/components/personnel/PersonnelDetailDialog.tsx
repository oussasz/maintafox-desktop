import { Building2, Users } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline, type TimelineEntry } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
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
  PersonnelTeamAssignment,
  PersonnelWorkHistoryEntry,
  PersonnelWorkloadSummary,
  SkillMatrixRow,
} from "@shared/ipc-types";

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

export function PersonnelDetailDialog() {
  const { t } = useTranslation("personnel");
  const activePersonnel = usePersonnelStore((s) => s.activePersonnel);
  const closePersonnel = usePersonnelStore((s) => s.closePersonnel);

  const open = activePersonnel !== null;
  const p = activePersonnel?.personnel ?? null;

  const [skills, setSkills] = useState<SkillMatrixRow[]>([]);
  const [calendarRows, setCalendarRows] = useState<AvailabilityCalendarEntry[]>([]);
  const [teamAssignments, setTeamAssignments] = useState<PersonnelTeamAssignment[]>([]);
  const [blocks, setBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [workHistory, setWorkHistory] = useState<PersonnelWorkHistoryEntry[]>([]);
  const [assignmentHistory, setAssignmentHistory] = useState<PersonnelAssignmentHistoryEntry[]>([]);
  const [workload, setWorkload] = useState<PersonnelWorkloadSummary | null>(null);
  const [tabsLoading, setTabsLoading] = useState(false);

  useEffect(() => {
    if (!p) return;
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
        /* individual tab renders handle empty state */
      })
      .finally(() => setTabsLoading(false));
  }, [p]);

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

  const isExternal = p?.employment_origin === "external";
  const isContractor = p?.employment_type === "contractor" || p?.employment_type === "vendor";

  return (
    <Dialog open={open} onOpenChange={(v) => !v && closePersonnel()}>
      <DialogContent className="max-h-[85vh] overflow-auto sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t("detail.title")}
            {p ? (
              <span className="font-mono text-sm font-normal text-muted-foreground">
                {p.employee_code}
              </span>
            ) : null}
          </DialogTitle>
          {p ? <p className="text-sm text-muted-foreground">{p.full_name}</p> : null}
        </DialogHeader>

        {isContractor && isExternal ? (
          <div className="rounded border border-amber-400/50 bg-amber-100/40 px-3 py-2 text-sm text-amber-900">
            {t("detail.contractorBanner", {
              company: p?.company_name ?? "—",
              defaultValue: "External contractor — {{company}}",
            })}
          </div>
        ) : null}

        <Tabs defaultValue="identity" className="w-full">
          <TabsList className="flex flex-wrap gap-1 h-auto">
            <TabsTrigger value="identity">{t("detail.tabs.identity")}</TabsTrigger>
            <TabsTrigger value="skills">{t("detail.tabs.skills")}</TabsTrigger>
            <TabsTrigger value="availability">{t("detail.tabs.availability")}</TabsTrigger>
            <TabsTrigger value="teams">{t("detail.tabs.teams")}</TabsTrigger>
            <TabsTrigger value="rates">{t("detail.tabs.rates")}</TabsTrigger>
            <TabsTrigger value="auth">{t("detail.tabs.authorizations")}</TabsTrigger>
            <TabsTrigger value="assignment">
              {t("detail.tabs.assignment", "Assignments")}
            </TabsTrigger>
            <TabsTrigger value="history">{t("detail.tabs.history")}</TabsTrigger>
            <TabsTrigger value="workload">{t("detail.tabs.workload")}</TabsTrigger>
          </TabsList>

          {/* ── Identity ─────────────────────────────────────────────── */}
          <TabsContent value="identity" className="space-y-2 pt-3 text-sm">
            <div className="grid gap-x-6 gap-y-1.5 sm:grid-cols-2">
              <Row label={t("field.position")}>
                {p?.position_code ? (
                  <span className="font-mono text-xs mr-1">{p.position_code}</span>
                ) : null}
                {p?.position_name ?? "—"}
              </Row>
              <Row label={t("field.entity")}>{p?.entity_name ?? "—"}</Row>
              <Row label={t("field.team")}>{p?.team_name ?? "—"}</Row>
              <Row label={t("field.schedule")}>{p?.schedule_name ?? "—"}</Row>
              <Row label={t("field.supervisor")}>{p?.supervisor_name ?? "—"}</Row>
              <Row label={t("field.employmentOrigin", "Origin")}>
                <Badge variant="outline" className="text-[10px] capitalize">
                  {p?.employment_origin
                    ? t(`employmentOrigin.${p.employment_origin}`, p.employment_origin)
                    : "—"}
                </Badge>
              </Row>
              <Row label={t("field.employmentStatus", "Status")}>
                <Badge variant="outline" className="text-[10px] capitalize">
                  {p?.employment_status
                    ? t(`employmentStatus.${p.employment_status}`, p.employment_status)
                    : "—"}
                </Badge>
              </Row>
              <Row label={t("field.hireDate")}>{fmt(p?.hire_date)}</Row>
              {isExternal ? (
                <>
                  <Row label={t("field.company")}>{p?.company_name ?? "—"}</Row>
                  <Row label={t("field.contractNumber", "Contract #")}>
                    {p?.contract_number ?? "—"}
                  </Row>
                  <Row label={t("field.contractStart", "Contract start")}>
                    {fmt(p?.contract_start_date)}
                  </Row>
                  <Row label={t("field.contractEnd", "Contract end")}>
                    {fmt(p?.contract_end_date)}
                  </Row>
                </>
              ) : null}
              <Row label={t("field.email")}>{p?.email ?? "—"}</Row>
              <Row label={t("field.phone")}>{p?.phone ?? "—"}</Row>
            </div>
          </TabsContent>

          {/* ── Skills ───────────────────────────────────────────────── */}
          <TabsContent value="skills" className="space-y-2 pt-3 text-sm">
            {tabsLoading ? <EmptyMsg msg={t("common.loading")} /> : null}
            {!tabsLoading && skills.length === 0 ? <EmptyMsg msg={t("common.noData")} /> : null}
            {skills.map((row) => (
              <div
                key={`${row.personnel_id}-${row.skill_code ?? "x"}`}
                className="rounded border p-2"
              >
                <div className="font-medium">{row.skill_label ?? "—"}</div>
                <div className="text-muted-foreground">
                  {t("skills.columns.level")}: {row.proficiency_level ?? "—"} ·{" "}
                  {t(`skills.coverage.${row.coverage_status}`, row.coverage_status)}
                </div>
              </div>
            ))}
          </TabsContent>

          {/* ── Availability ─────────────────────────────────────────── */}
          <TabsContent value="availability" className="space-y-3 pt-3 text-sm">
            {calendarByDate.length === 0 && !tabsLoading ? (
              <EmptyMsg msg={t("common.noData")} />
            ) : null}
            <div className="grid gap-2 md:grid-cols-2">
              {calendarByDate.map((row) => (
                <div key={row.work_date} className="rounded border p-2">
                  <div className="font-medium">{row.work_date}</div>
                  <div className="text-muted-foreground">
                    {row.available_minutes}m {t("detail.available", "available")} /{" "}
                    {row.blocked_minutes}m {t("detail.blocked", "blocked")}
                  </div>
                </div>
              ))}
            </div>
            {blocks.map((b) => (
              <div key={b.id} className="rounded border p-2 text-sm">
                <span className="font-medium">{b.block_type}</span> · {fmt(b.start_at)} →{" "}
                {fmt(b.end_at)}
              </div>
            ))}
          </TabsContent>

          {/* ── Teams ────────────────────────────────────────────────── */}
          <TabsContent value="teams" className="space-y-2 pt-3 text-sm">
            {!tabsLoading && teamAssignments.length === 0 ? (
              <EmptyMsg msg={t("common.noData")} />
            ) : null}
            {teamAssignments.map((ta) => (
              <div key={ta.id} className="rounded border p-2">
                <div className="font-medium">{ta.team_name ?? "—"}</div>
                <div className="text-muted-foreground">
                  {ta.role_code} · {Math.round(ta.allocation_percent)}%
                </div>
              </div>
            ))}
          </TabsContent>

          {/* ── Rate cards ───────────────────────────────────────────── */}
          <TabsContent value="rates" className="space-y-2 pt-3 text-sm">
            {(activePersonnel?.rate_cards ?? []).length === 0 ? (
              <EmptyMsg msg={t("common.noData")} />
            ) : null}
            {(activePersonnel?.rate_cards ?? []).map((rc) => (
              <div key={rc.id} className="rounded border p-2">
                {rc.effective_from} · {rc.labor_rate.toFixed(2)} / {rc.overtime_rate.toFixed(2)}
              </div>
            ))}
          </TabsContent>

          {/* ── Authorizations ───────────────────────────────────────── */}
          <TabsContent value="auth" className="space-y-2 pt-3 text-sm">
            {(activePersonnel?.authorizations ?? []).length === 0 ? (
              <EmptyMsg msg={t("common.noData")} />
            ) : null}
            {(activePersonnel?.authorizations ?? []).map((a) => (
              <div key={a.id} className="rounded border p-2">
                {a.authorization_type} · {a.valid_from} → {a.valid_to ?? "—"}
              </div>
            ))}
          </TabsContent>

          {/* ── Assignment History ────────────────────────── (Timeline) */}
          <TabsContent value="assignment" className="pt-3">
            <Timeline
              items={assignmentEntries}
              loading={tabsLoading}
              aria-label={t("detail.tabs.assignment", "Assignments")}
              empty={t("common.noData")}
              groupBy="day"
              todayLabel={t("common.today", "Today")}
            />
          </TabsContent>

          {/* ── Work History ─────────────────────────────── (Timeline) */}
          <TabsContent value="history" className="pt-3">
            <Timeline
              items={workEntries}
              loading={tabsLoading}
              aria-label={t("detail.tabs.history")}
              empty={t("common.noData")}
              groupBy="day"
              todayLabel={t("common.today", "Today")}
            />
          </TabsContent>

          {/* ── Workload ─────────────────────────────────────────────── */}
          <TabsContent value="workload" className="space-y-2 pt-3 text-sm">
            {tabsLoading ? <EmptyMsg msg={t("common.loading")} /> : null}
            {workload ? (
              <div className="grid gap-1.5 sm:grid-cols-2">
                <Row label={t("detail.workload.openWo")}>{workload.open_work_orders}</Row>
                <Row label={t("detail.workload.inProgressWo")}>
                  {workload.in_progress_work_orders}
                </Row>
                <Row label={t("detail.workload.pendingDi")}>{workload.pending_interventions}</Row>
                <Row label={t("detail.workload.di30d")}>{workload.interventions_last_30d}</Row>
              </div>
            ) : null}
            {!tabsLoading && !workload ? <EmptyMsg msg={t("common.noData")} /> : null}
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <span className="text-muted-foreground">{label}: </span>
      {children}
    </div>
  );
}

function EmptyMsg({ msg }: { msg: string }) {
  return <p className="text-sm text-muted-foreground">{msg}</p>;
}

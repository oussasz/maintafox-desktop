import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  getPersonnelWorkloadSummary,
  listAvailabilityCalendar,
  listPersonnelAvailabilityBlocks,
  listPersonnelTeamAssignments,
  listPersonnelWorkHistory,
  listSkillsMatrix,
} from "@/services/personnel-service";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usePersonnelStore } from "@/stores/personnel-store";
import type {
  AvailabilityCalendarEntry,
  PersonnelAvailabilityBlock,
  PersonnelTeamAssignment,
  PersonnelWorkHistoryEntry,
  PersonnelWorkloadSummary,
  SkillMatrixRow,
} from "@shared/ipc-types";

function dateRange(days = 7) {
  const from = new Date();
  const to = new Date();
  to.setDate(to.getDate() + days);
  return {
    from: from.toISOString().slice(0, 10),
    to: to.toISOString().slice(0, 10),
  };
}

export function PersonnelDetailDialog() {
  const { t } = useTranslation("personnel");
  const activePersonnel = usePersonnelStore((s) => s.activePersonnel);
  const closePersonnel = usePersonnelStore((s) => s.closePersonnel);

  const open = activePersonnel !== null;
  const p = activePersonnel?.personnel;
  const detail = activePersonnel;
  const [skills, setSkills] = useState<SkillMatrixRow[]>([]);
  const [calendarRows, setCalendarRows] = useState<AvailabilityCalendarEntry[]>([]);
  const [teamAssignments, setTeamAssignments] = useState<PersonnelTeamAssignment[]>([]);
  const [blocks, setBlocks] = useState<PersonnelAvailabilityBlock[]>([]);
  const [workHistory, setWorkHistory] = useState<PersonnelWorkHistoryEntry[]>([]);
  const [workload, setWorkload] = useState<PersonnelWorkloadSummary | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !p) return;
    const range = dateRange(14);
    setLoading(true);
    void Promise.all([
      listSkillsMatrix({ personnel_id: p.id, include_inactive: true }),
      listAvailabilityCalendar({
        date_from: range.from,
        date_to: range.to,
        personnel_id: p.id,
        include_inactive: true,
      }),
      listPersonnelTeamAssignments(p.id),
      listPersonnelAvailabilityBlocks(p.id, 20),
      listPersonnelWorkHistory(p.id, 40),
      getPersonnelWorkloadSummary(p.id),
    ])
      .then(([skillsData, availData, teamsData, blocksData, historyData, summaryData]) => {
        setSkills(skillsData);
        setCalendarRows(availData);
        setTeamAssignments(teamsData);
        setBlocks(blocksData);
        setWorkHistory(historyData);
        setWorkload(summaryData);
      })
      .catch(() => {
        setSkills([]);
        setCalendarRows([]);
        setTeamAssignments([]);
        setBlocks([]);
        setWorkHistory([]);
        setWorkload(null);
      })
      .finally(() => setLoading(false));
  }, [open, p]);

  const isContractor = p?.employment_type === "contractor" || p?.employment_type === "vendor";
  const availabilityByDate = useMemo(() => {
    const out = new Map<string, AvailabilityCalendarEntry>();
    for (const row of calendarRows) out.set(row.work_date, row);
    return [...out.entries()];
  }, [calendarRows]);

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && closePersonnel()}>
      <DialogContent className="max-h-[85vh] overflow-auto sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>
            {t("detail.title")}
            {p ? <span className="ml-2 text-sm font-mono text-muted-foreground">{p.employee_code}</span> : null}
          </DialogTitle>
          {p ? <div className="text-sm text-muted-foreground">{p.full_name}</div> : null}
        </DialogHeader>

        {isContractor ? (
          <div className="rounded border border-amber-400/50 bg-amber-100/40 px-3 py-2 text-sm text-amber-900">
            {t("detail.contractorBanner", { company: p?.company_name ?? "—" })}
          </div>
        ) : null}

        <Tabs defaultValue="identity" className="w-full">
          <TabsList className="grid w-full grid-cols-4 lg:grid-cols-8">
            <TabsTrigger value="identity">{t("detail.tabs.identity")}</TabsTrigger>
            <TabsTrigger value="skills">{t("detail.tabs.skills")}</TabsTrigger>
            <TabsTrigger value="availability">{t("detail.tabs.availability")}</TabsTrigger>
            <TabsTrigger value="teams">{t("detail.tabs.teams")}</TabsTrigger>
            <TabsTrigger value="rates">{t("detail.tabs.rates")}</TabsTrigger>
            <TabsTrigger value="auth">{t("detail.tabs.authorizations")}</TabsTrigger>
            <TabsTrigger value="history">{t("detail.tabs.history")}</TabsTrigger>
            <TabsTrigger value="workload">{t("detail.tabs.workload")}</TabsTrigger>
          </TabsList>

          <TabsContent value="identity" className="space-y-2 pt-3 text-sm">
            <div>{t("field.position")}: {p?.position_name ?? "—"}</div>
            <div>{t("field.entity")}: {p?.entity_name ?? "—"}</div>
            <div>{t("field.team")}: {p?.team_name ?? "—"}</div>
            <div>{t("field.schedule")}: {p?.schedule_name ?? "—"}</div>
            <div>{t("field.email")}: {p?.email ?? "—"}</div>
            <div>{t("field.phone")}: {p?.phone ?? "—"}</div>
          </TabsContent>

          <TabsContent value="skills" className="space-y-2 pt-3 text-sm">
            {skills.length === 0 ? <div className="text-muted-foreground">{t("common.noData")}</div> : null}
            {skills.map((row) => (
              <div key={`${row.personnel_id}-${row.skill_code ?? "none"}`} className="rounded border p-2">
                <div className="font-medium">{row.skill_label ?? "—"}</div>
                <div className="text-muted-foreground">
                  {t("skills.columns.level")}: {row.proficiency_level ?? "—"} · {t(`skills.coverage.${row.coverage_status}`)}
                </div>
              </div>
            ))}
          </TabsContent>

          <TabsContent value="availability" className="space-y-3 pt-3 text-sm">
            <div className="grid gap-2 md:grid-cols-2">
              {availabilityByDate.map(([day, row]) => (
                <div key={day} className="rounded border p-2">
                  <div className="font-medium">{day}</div>
                  <div className="text-muted-foreground">
                    {row.available_minutes}m {t("detail.available")} / {row.blocked_minutes}m {t("detail.blocked")}
                  </div>
                </div>
              ))}
            </div>
            <div className="space-y-1">
              {blocks.map((b) => (
                <div key={b.id} className="rounded border p-2">
                  <span className="font-medium">{b.block_type}</span> · {b.start_at} → {b.end_at}
                </div>
              ))}
            </div>
          </TabsContent>

          <TabsContent value="teams" className="space-y-2 pt-3 text-sm">
            {teamAssignments.map((ta) => (
              <div key={ta.id} className="rounded border p-2">
                <div className="font-medium">{ta.team_name ?? "—"}</div>
                <div className="text-muted-foreground">
                  {ta.role_code} · {Math.round(ta.allocation_percent)}%
                </div>
              </div>
            ))}
            {teamAssignments.length === 0 ? <div className="text-muted-foreground">{t("common.noData")}</div> : null}
          </TabsContent>

          <TabsContent value="rates" className="space-y-2 pt-3 text-sm">
            {(detail?.rate_cards ?? []).map((rc) => (
              <div key={rc.id} className="rounded border p-2">
                {rc.effective_from} · {rc.labor_rate.toFixed(2)} / {rc.overtime_rate.toFixed(2)}
              </div>
            ))}
            {(detail?.rate_cards ?? []).length === 0 ? (
              <div className="text-muted-foreground">{t("common.noData")}</div>
            ) : null}
          </TabsContent>

          <TabsContent value="auth" className="space-y-2 pt-3 text-sm">
            {(detail?.authorizations ?? []).map((a) => (
              <div key={a.id} className="rounded border p-2">
                {a.authorization_type} · {a.valid_from} → {a.valid_to ?? "—"}
              </div>
            ))}
            {(detail?.authorizations ?? []).length === 0 ? (
              <div className="text-muted-foreground">{t("common.noData")}</div>
            ) : null}
          </TabsContent>

          <TabsContent value="history" className="space-y-2 pt-3 text-sm">
            {workHistory.map((h) => (
              <div key={`${h.source_module}-${h.record_id}-${h.role_code}`} className="rounded border p-2">
                <div className="font-medium">
                  {h.source_module.toUpperCase()} {h.record_code ?? h.record_id}
                </div>
                <div className="text-muted-foreground">
                  {h.role_code} · {h.status_code ?? "—"} · {h.happened_at}
                </div>
              </div>
            ))}
            {workHistory.length === 0 ? <div className="text-muted-foreground">{t("common.noData")}</div> : null}
          </TabsContent>

          <TabsContent value="workload" className="space-y-2 pt-3 text-sm">
            {loading ? <div className="text-muted-foreground">{t("common.loading")}</div> : null}
            {workload ? (
              <>
                <div>{t("detail.workload.openWo")}: {workload.open_work_orders}</div>
                <div>{t("detail.workload.inProgressWo")}: {workload.in_progress_work_orders}</div>
                <div>{t("detail.workload.pendingDi")}: {workload.pending_interventions}</div>
                <div>{t("detail.workload.di30d")}: {workload.interventions_last_30d}</div>
              </>
            ) : null}
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

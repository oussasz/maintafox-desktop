import { Bell, CalendarRange, Download, Lock, RefreshCw, Search, Siren, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { listOrgTree } from "@/services/org-node-service";
import { listPersonnel } from "@/services/personnel-service";
import { usePlanningStore } from "@/stores/planning-store";
import { toErrorMessage } from "@/utils/errors";

type OrgTeamOption = { id: number; label: string };
type PersonnelOption = { id: number; label: string };

function toDateTimeLocal(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function fromDateTimeLocal(local: string): string {
  const d = new Date(local);
  return Number.isNaN(d.getTime()) ? local : d.toISOString();
}

function buildDayBuckets(periodStartIso: string, periodEndIso: string): string[] {
  const start = new Date(periodStartIso);
  const end = new Date(periodEndIso);
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime()) || end <= start) return [];
  const cursor = new Date(start);
  cursor.setUTCHours(0, 0, 0, 0);
  const final = new Date(end);
  final.setUTCHours(0, 0, 0, 0);
  const days: string[] = [];
  while (cursor <= final) {
    days.push(cursor.toISOString().slice(0, 10));
    cursor.setUTCDate(cursor.getUTCDate() + 1);
  }
  return days;
}

export function PlanningPage() {
  const { t } = useTranslation("planning");

  const backlog = usePlanningStore((s) => s.backlog);
  const commitments = usePlanningStore((s) => s.commitments);
  const planningWindows = usePlanningStore((s) => s.planningWindows);
  const breakIns = usePlanningStore((s) => s.breakIns);
  const capacityLoad = usePlanningStore((s) => s.capacityLoad);
  const ganttSnapshot = usePlanningStore((s) => s.ganttSnapshot);
  const loading = usePlanningStore((s) => s.loading);
  const saving = usePlanningStore((s) => s.saving);
  const storeError = usePlanningStore((s) => s.error);

  const loadBacklog = usePlanningStore((s) => s.loadBacklog);
  const refreshBacklog = usePlanningStore((s) => s.refreshBacklog);
  const loadGantt = usePlanningStore((s) => s.loadGantt);
  const loadCommitments = usePlanningStore((s) => s.loadCommitments);
  const loadPlanningWindows = usePlanningStore((s) => s.loadPlanningWindows);
  const loadBreakIns = usePlanningStore((s) => s.loadBreakIns);
  const loadCapacityLoad = usePlanningStore((s) => s.loadCapacityLoad);
  const createCommitment = usePlanningStore((s) => s.createCommitment);
  const rescheduleCommitment = usePlanningStore((s) => s.rescheduleCommitment);
  const createPlanningWindow = usePlanningStore((s) => s.createPlanningWindow);
  const createBreakIn = usePlanningStore((s) => s.createBreakIn);
  const freezeSchedule = usePlanningStore((s) => s.freezeSchedule);
  const notifyTeams = usePlanningStore((s) => s.notifyTeams);
  const exportGanttPdf = usePlanningStore((s) => s.exportGanttPdf);

  const [teams, setTeams] = useState<OrgTeamOption[]>([]);
  const [personnel, setPersonnel] = useState<PersonnelOption[]>([]);
  const [pageError, setPageError] = useState<string | null>(null);
  const [dragCommitmentId, setDragCommitmentId] = useState<number | null>(null);

  const [teamId, setTeamId] = useState<number | null>(null);
  const [selectedCandidateId, setSelectedCandidateId] = useState<number | null>(null);
  const [selectedPersonnelId, setSelectedPersonnelId] = useState<number | null>(null);
  const [commitStart, setCommitStart] = useState(toDateTimeLocal(new Date().toISOString()));
  const [commitEnd, setCommitEnd] = useState(toDateTimeLocal(new Date(Date.now() + 2 * 3600_000).toISOString()));
  const [budgetThreshold, setBudgetThreshold] = useState("0");
  const [periodStart, setPeriodStart] = useState(toDateTimeLocal(new Date().toISOString()));
  const [periodEnd, setPeriodEnd] = useState(toDateTimeLocal(new Date(Date.now() + 6 * 24 * 3600_000).toISOString()));
  const [windowStart, setWindowStart] = useState(toDateTimeLocal(new Date().toISOString()));
  const [windowEnd, setWindowEnd] = useState(toDateTimeLocal(new Date(Date.now() + 24 * 3600_000).toISOString()));
  const [windowType, setWindowType] = useState("production");
  const [windowReason, setWindowReason] = useState("");
  const [breakInCommitmentId, setBreakInCommitmentId] = useState<number | null>(null);
  const [breakInReason, setBreakInReason] = useState("emergency");
  const [breakInApproverUserId, setBreakInApproverUserId] = useState("");
  const [breakInOverrideReason, setBreakInOverrideReason] = useState("");
  const [searchInput, setSearchInput] = useState("");

  useEffect(() => {
    void loadBacklog();
  }, [loadBacklog]);

  useEffect(() => {
    void (async () => {
      try {
        const [orgRows, peoplePage] = await Promise.all([
          listOrgTree(),
          listPersonnel({ limit: 250, offset: 0 }),
        ]);
        setTeams(
          orgRows.map((row) => ({
            id: row.node.id,
            label: row.node.code ? `${row.node.name} (${row.node.code})` : row.node.name,
          })),
        );
        setPersonnel(
          peoplePage.items.map((row) => ({
            id: row.id,
            label: row.employee_code ? `${row.full_name} (${row.employee_code})` : row.full_name,
          })),
        );
        const firstTeam = orgRows[0];
        if (firstTeam) setTeamId((prev) => prev ?? firstTeam.node.id);
      } catch (err) {
        setPageError(toErrorMessage(err));
      }
    })();
  }, []);

  useEffect(() => {
    if (!teamId) return;
    const startIso = fromDateTimeLocal(periodStart);
    const endIso = fromDateTimeLocal(periodEnd);
    void loadCommitments({
      period_start: startIso,
      period_end: endIso,
      team_id: teamId,
    });
    void loadCapacityLoad(startIso, endIso, teamId);
    void loadPlanningWindows({ include_locked: true });
    void loadBreakIns({
      period_start: startIso,
      period_end: endIso,
    });
    void loadGantt({
      period_start: startIso,
      period_end: endIso,
      team_id: teamId,
    });
  }, [teamId, periodStart, periodEnd, loadCommitments, loadCapacityLoad, loadGantt, loadPlanningWindows, loadBreakIns]);

  const readyCandidates = useMemo(
    () =>
      backlog?.candidates.filter(
        (candidate: (typeof backlog.candidates)[number]) => candidate.readiness_status === "ready",
      ) ?? [],
    [backlog],
  );
  const blockedCandidates = useMemo(
    () =>
      backlog?.candidates.filter(
        (candidate: (typeof backlog.candidates)[number]) => candidate.readiness_status !== "ready",
      ) ?? [],
    [backlog],
  );
  const normalizedSearch = searchInput.trim().toLowerCase();
  const filteredReadyCandidates = useMemo(
    () =>
      readyCandidates.filter((candidate) => {
        if (!normalizedSearch) return true;
        return `${candidate.source_type}:${candidate.source_id}`.toLowerCase().includes(normalizedSearch);
      }),
    [readyCandidates, normalizedSearch],
  );
  const filteredBlockedCandidates = useMemo(
    () =>
      blockedCandidates.filter((candidate) => {
        if (!normalizedSearch) return true;
        return `${candidate.source_type}:${candidate.source_id}`.toLowerCase().includes(normalizedSearch);
      }),
    [blockedCandidates, normalizedSearch],
  );
  const dayBuckets = useMemo(
    () => buildDayBuckets(fromDateTimeLocal(periodStart), fromDateTimeLocal(periodEnd)),
    [periodStart, periodEnd],
  );
  const teamValue = teamId ? String(teamId) : "__none__";
  const personnelValue = selectedPersonnelId ? String(selectedPersonnelId) : "__unassigned__";
  const breakInCommitmentValue = breakInCommitmentId ? String(breakInCommitmentId) : "__none__";

  async function handleCreateCommitment() {
    if (!selectedCandidateId || !teamId) return;
    setPageError(null);
    try {
      await createCommitment({
        schedule_candidate_id: selectedCandidateId,
        committed_start: fromDateTimeLocal(commitStart),
        committed_end: fromDateTimeLocal(commitEnd),
        assigned_team_id: teamId,
        assigned_personnel_id: selectedPersonnelId,
        allow_double_booking_override: false,
        budget_threshold: Number.isFinite(Number(budgetThreshold)) ? Number(budgetThreshold) : null,
      });
      await refreshBacklog();
      await loadCommitments({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
      await loadGantt({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  async function handleDropCommitment(day: string, personnelId: number | null) {
    if (!dragCommitmentId || !teamId) return;
    const commitment = commitments.find((entry) => entry.id === dragCommitmentId);
    if (!commitment) return;
    setPageError(null);
    try {
      const oldStart = new Date(commitment.committed_start);
      const oldEnd = new Date(commitment.committed_end);
      const durationMs = oldEnd.getTime() - oldStart.getTime();
      const nextStart = new Date(`${day}T${oldStart.toISOString().slice(11, 19)}Z`);
      const nextEnd = new Date(nextStart.getTime() + durationMs);

      try {
        await rescheduleCommitment({
          commitment_id: commitment.id,
          expected_row_version: commitment.row_version,
          committed_start: nextStart.toISOString(),
          committed_end: nextEnd.toISOString(),
          assigned_team_id: teamId,
          assigned_personnel_id: personnelId,
          allow_double_booking_override: false,
          budget_threshold: commitment.budget_threshold,
        });
      } catch (firstErr) {
        const text = toErrorMessage(firstErr);
        if (!text.toLowerCase().includes("double-booking")) throw firstErr;
        const reason = window.prompt(t("gantt.overrideReasonPrompt"));
        if (!reason) throw firstErr;
        await rescheduleCommitment({
          commitment_id: commitment.id,
          expected_row_version: commitment.row_version,
          committed_start: nextStart.toISOString(),
          committed_end: nextEnd.toISOString(),
          assigned_team_id: teamId,
          assigned_personnel_id: personnelId,
          allow_double_booking_override: true,
          override_reason: reason,
          budget_threshold: commitment.budget_threshold,
        });
      }

      await loadCommitments({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
      await loadGantt({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    } finally {
      setDragCommitmentId(null);
    }
  }

  async function handleFreeze() {
    setPageError(null);
    try {
      await freezeSchedule({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        reason: t("freeze.defaultReason"),
      });
      await loadCommitments({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
      await loadGantt({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  async function handleCreateWindow() {
    setPageError(null);
    try {
      await createPlanningWindow({
        entity_id: teamId,
        window_type: windowType,
        start_datetime: fromDateTimeLocal(windowStart),
        end_datetime: fromDateTimeLocal(windowEnd),
        is_locked: true,
        lock_reason: windowReason || null,
      });
      await loadPlanningWindows({ include_locked: true });
      await loadGantt({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  async function handleCreateBreakIn() {
    if (!breakInCommitmentId) return;
    const sourceCommitment = commitments.find((entry) => entry.id === breakInCommitmentId);
    if (!sourceCommitment) return;
    setPageError(null);
    try {
      await createBreakIn({
        schedule_commitment_id: sourceCommitment.id,
        expected_commitment_row_version: sourceCommitment.row_version,
        break_in_reason: breakInReason,
        approved_by_user_id: breakInApproverUserId ? Number(breakInApproverUserId) : null,
        new_slot_start: sourceCommitment.committed_start,
        new_slot_end: sourceCommitment.committed_end,
        new_assigned_team_id: teamId,
        new_assigned_personnel_id: sourceCommitment.assigned_personnel_id,
        bypass_availability: false,
        bypass_qualification: false,
        override_reason: breakInOverrideReason || null,
        dangerous_override_reason: breakInOverrideReason || null,
      });
      await loadBreakIns({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
      });
      await loadCommitments({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  async function handleNotifyTeams() {
    setPageError(null);
    try {
      await notifyTeams({
        period_start: fromDateTimeLocal(periodStart),
        period_end: fromDateTimeLocal(periodEnd),
        team_id: teamId,
        include_break_ins: true,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  function clearSearch() {
    setSearchInput("");
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-6 py-3">
        <div className="flex items-center gap-3">
          <CalendarRange className="h-5 w-5 text-text-muted" aria-hidden />
          <h1 className="text-xl font-semibold text-text-primary">{t("page.title", "Planning")}</h1>
          <Badge variant="secondary" className="text-xs">
            {backlog?.candidate_count ?? commitments.length}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          <PermissionGate permission="plan.edit">
            <Button size="sm" variant="outline" onClick={() => void refreshBacklog()} disabled={saving} className="gap-1.5">
              <RefreshCw className="h-3.5 w-3.5" />
              {t("actions.refreshBacklog")}
            </Button>
          </PermissionGate>
          <PermissionGate permission="plan.confirm">
            <Button size="sm" variant="outline" onClick={() => void handleFreeze()} disabled={saving} className="gap-1.5">
              <Lock className="h-3.5 w-3.5" />
              {t("actions.freezePeriod")}
            </Button>
          </PermissionGate>
          <PermissionGate permission="plan.confirm">
            <Button size="sm" variant="outline" onClick={() => void handleNotifyTeams()} disabled={saving} className="gap-1.5">
              <Bell className="h-3.5 w-3.5" />
              {t("actions.notifyTeams")}
            </Button>
          </PermissionGate>
          <Button size="sm" variant="outline" onClick={() => void exportGanttPdf(fromDateTimeLocal(periodStart), fromDateTimeLocal(periodEnd), teamId)} className="gap-1.5">
            <Download className="h-3.5 w-3.5" />
            {t("actions.exportPdf")}
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 border-b border-surface-border px-6 py-2">
        <div className="relative max-w-sm flex-1">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            className="h-8 pl-9 pr-8 text-sm"
            placeholder={t("filters.searchPlaceholder", "Search planning backlog")}
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
          />
          {searchInput ? (
            <button
              type="button"
              className="absolute right-2 top-2 text-text-muted hover:text-text-primary"
              onClick={clearSearch}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </div>

        <Select value={teamValue} onValueChange={(value) => setTeamId(value === "__none__" ? null : Number(value))}>
          <SelectTrigger className="h-8 w-[220px] text-sm">
            <SelectValue placeholder={t("filters.team")} />
          </SelectTrigger>
          <SelectContent>
            {teams.map((team) => (
              <SelectItem key={team.id} value={String(team.id)}>
                {team.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Input className="h-8 w-[200px] text-sm" type="datetime-local" value={periodStart} onChange={(e) => setPeriodStart(e.target.value)} />
        <Input className="h-8 w-[200px] text-sm" type="datetime-local" value={periodEnd} onChange={(e) => setPeriodEnd(e.target.value)} />

        <Button type="button" variant="ghost" size="sm" className="h-8 text-sm" onClick={clearSearch}>
          {t("filters.clearAll", "Clear")}
        </Button>
      </div>

      {(pageError || storeError) && (
        <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive">{pageError ?? storeError}</div>
      )}

      <div className="flex min-h-0 flex-1 flex-col overflow-auto p-4">
        <div className="grid gap-4 xl:grid-cols-2">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("backlog.readyColumn")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2">
              {filteredReadyCandidates.map((candidate: (typeof filteredReadyCandidates)[number]) => (
              <button
                key={candidate.id}
                type="button"
                className={`w-full rounded-lg border p-3 text-left text-sm transition-colors hover:bg-muted/40 ${selectedCandidateId === candidate.id ? "border-primary bg-muted/30" : "border-surface-border"}`}
                onClick={() => {
                  setSelectedCandidateId(candidate.id);
                  setSelectedPersonnelId(candidate.assigned_personnel_id);
                }}
              >
                <div className="font-medium text-text-primary">
                  {candidate.source_type}:{candidate.source_id}
                </div>
                <div className="text-xs text-text-muted">
                  {t("backlog.readinessScore")}: {candidate.readiness_score.toFixed(1)}
                </div>
              </button>
              ))}
              {filteredReadyCandidates.length === 0 && <p className="text-xs text-text-muted">{t("backlog.emptyReady")}</p>}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("backlog.blockedColumn")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2">
              {filteredBlockedCandidates.slice(0, 10).map((candidate: (typeof filteredBlockedCandidates)[number]) => (
              <div key={candidate.id} className="rounded-lg border border-surface-border p-3 text-sm">
                <div className="font-medium text-text-primary">
                  {candidate.source_type}:{candidate.source_id}
                </div>
                <div className="text-xs text-text-muted">
                  {t("backlog.readinessScore")}: {candidate.readiness_score.toFixed(1)}
                </div>
              </div>
              ))}
              {filteredBlockedCandidates.length === 0 && <p className="text-xs text-text-muted">{t("backlog.emptyBlocked")}</p>}
            </CardContent>
          </Card>
        </div>

        <div className="mt-4 grid gap-4 2xl:grid-cols-[1.1fr_1fr]">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("commitment.formTitle")}</CardTitle>
            </CardHeader>
            <CardContent>
        <div className="grid gap-3 md:grid-cols-5">
          <div>
            <Label>{t("commitment.selectedCandidate")}</Label>
            <Input className="h-8 text-sm" value={selectedCandidateId ?? ""} readOnly />
          </div>
          <div>
            <Label>{t("commitment.start")}</Label>
            <Input className="h-8 text-sm" type="datetime-local" value={commitStart} onChange={(e) => setCommitStart(e.target.value)} />
          </div>
          <div>
            <Label>{t("commitment.end")}</Label>
            <Input className="h-8 text-sm" type="datetime-local" value={commitEnd} onChange={(e) => setCommitEnd(e.target.value)} />
          </div>
          <div>
            <Label>{t("commitment.assignee")}</Label>
            <Select value={personnelValue} onValueChange={(value) => setSelectedPersonnelId(value === "__unassigned__" ? null : Number(value))}>
              <SelectTrigger className="h-8 text-sm">
                <SelectValue placeholder={t("commitment.assignee")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__unassigned__">{t("commitment.unassigned")}</SelectItem>
                {personnel.map((person) => (
                  <SelectItem key={person.id} value={String(person.id)}>
                    {person.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <Label>{t("commitment.budgetThreshold")}</Label>
            <Input className="h-8 text-sm" value={budgetThreshold} onChange={(e) => setBudgetThreshold(e.target.value)} />
          </div>
        </div>
        <PermissionGate permission="plan.confirm">
          <Button className="mt-3" size="sm" onClick={() => void handleCreateCommitment()} disabled={!selectedCandidateId || !teamId || saving}>
            {t("actions.commit")}
          </Button>
        </PermissionGate>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("windows.title")}</CardTitle>
            </CardHeader>
            <CardContent>
        <div className="grid gap-3 md:grid-cols-5">
          <div>
            <Label>{t("windows.type")}</Label>
            <Input className="h-8 text-sm" value={windowType} onChange={(e) => setWindowType(e.target.value)} />
          </div>
          <div>
            <Label>{t("windows.start")}</Label>
            <Input className="h-8 text-sm" type="datetime-local" value={windowStart} onChange={(e) => setWindowStart(e.target.value)} />
          </div>
          <div>
            <Label>{t("windows.end")}</Label>
            <Input className="h-8 text-sm" type="datetime-local" value={windowEnd} onChange={(e) => setWindowEnd(e.target.value)} />
          </div>
          <div className="md:col-span-2">
            <Label>{t("windows.reason")}</Label>
            <Input className="h-8 text-sm" value={windowReason} onChange={(e) => setWindowReason(e.target.value)} />
          </div>
        </div>
        <PermissionGate permission="plan.windows">
          <Button className="mt-3" size="sm" variant="outline" onClick={() => void handleCreateWindow()} disabled={saving}>
            {t("windows.add")}
          </Button>
        </PermissionGate>
        <div className="mt-3 space-y-1 text-xs">
          {planningWindows.map((window) => (
            <div key={window.id} className="rounded-lg border border-surface-border p-2">
              <span className="font-medium">{window.window_type}</span> {window.start_datetime} - {window.end_datetime}
              {window.is_locked === 1 && <Badge className="ml-2">{t("windows.locked")}</Badge>}
            </div>
          ))}
        </div>
            </CardContent>
          </Card>
        </div>

        <Card className="mt-4">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm">{t("gantt.capacityTitle")}</CardTitle>
          </CardHeader>
          <CardContent>
        <div className="grid gap-2 md:grid-cols-3">
          {capacityLoad.map((row) => {
            const tone = row.utilization_ratio > 1 ? "destructive" : row.utilization_ratio > 0.8 ? "secondary" : "default";
            return (
              <div key={`${row.team_id}-${row.work_date}`} className="rounded-lg border border-surface-border p-2 text-xs">
                <div className="font-medium">{row.work_date}</div>
                <div>
                  {t("gantt.capacityLine", {
                    committed: row.committed_hours.toFixed(1),
                    available: (row.available_hours + row.overtime_hours).toFixed(1),
                  })}
                </div>
                <Badge variant={tone as "default" | "secondary" | "destructive"}>
                  {(row.utilization_ratio * 100).toFixed(0)}%
                </Badge>
              </div>
            );
          })}
          {capacityLoad.length === 0 && <p className="text-xs text-text-muted">{t("gantt.emptyCapacity")}</p>}
        </div>
          </CardContent>
        </Card>

        <Card className="mt-4">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm">{t("gantt.assignmentLaneTitle")}</CardTitle>
          </CardHeader>
          <CardContent>
        {(ganttSnapshot?.locked_windows ?? []).length > 0 && (
          <div className="mb-3 rounded-lg border border-amber-300 bg-amber-50 p-2 text-xs">
            <div className="mb-1 font-medium">{t("gantt.freezeOverlayTitle")}</div>
            {(ganttSnapshot?.locked_windows ?? []).map((window) => (
              <div key={window.id}>
                {window.start_datetime} - {window.end_datetime} ({window.lock_reason ?? window.window_type})
              </div>
            ))}
          </div>
        )}
        <div className="overflow-x-auto">
          <div className="min-w-[980px] space-y-2">
            <div className="grid grid-cols-[220px_repeat(8,minmax(100px,1fr))] gap-1 text-xs font-semibold">
              <div>{t("gantt.laneHeader")}</div>
              {dayBuckets.slice(0, 8).map((day) => (
                <div key={day}>{day}</div>
              ))}
            </div>
            {(ganttSnapshot?.assignee_lanes ?? []).map((lane) => {
              const laneCommitments = commitments.filter((entry) => entry.assigned_personnel_id === lane.personnel_id);
              return (
                <div key={lane.personnel_id} className="grid grid-cols-[220px_repeat(8,minmax(100px,1fr))] gap-1">
                  <div className="rounded border px-2 py-1 text-xs">{lane.full_name}</div>
                  {dayBuckets.slice(0, 8).map((day) => (
                    <div
                      key={`${lane.personnel_id}-${day}`}
                      className="min-h-[64px] rounded-lg border border-surface-border p-1"
                      onDragOver={(e) => e.preventDefault()}
                      onDrop={() => void handleDropCommitment(day, lane.personnel_id)}
                    >
                      {laneCommitments
                        .filter((entry) => entry.schedule_period_start === day)
                        .map((entry) => (
                          <div
                            key={entry.id}
                            draggable
                            onDragStart={() => setDragCommitmentId(entry.id)}
                            className="mb-1 cursor-move rounded-md bg-blue-100 px-1 py-0.5 text-[11px]"
                          >
                            #{entry.id} {entry.source_type}:{entry.source_id}
                          </div>
                        ))}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        </div>
          </CardContent>
        </Card>

        <Card className="mt-4">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm">{t("breakIns.title")}</CardTitle>
          </CardHeader>
          <CardContent>
        <div className="grid gap-3 md:grid-cols-5">
          <div>
            <Label>{t("breakIns.commitment")}</Label>
            <Select value={breakInCommitmentValue} onValueChange={(value) => setBreakInCommitmentId(value === "__none__" ? null : Number(value))}>
              <SelectTrigger className="h-8 text-sm">
                <SelectValue placeholder={t("breakIns.selectCommitment")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("breakIns.selectCommitment")}</SelectItem>
                {commitments.map((commitment) => (
                  <SelectItem key={commitment.id} value={String(commitment.id)}>
                    #{commitment.id} {commitment.source_type}:{commitment.source_id}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <Label>{t("breakIns.reason")}</Label>
            <Input className="h-8 text-sm" value={breakInReason} onChange={(e) => setBreakInReason(e.target.value)} />
          </div>
          <div>
            <Label>{t("breakIns.approver")}</Label>
            <Input className="h-8 text-sm" value={breakInApproverUserId} onChange={(e) => setBreakInApproverUserId(e.target.value)} />
          </div>
          <div className="md:col-span-2">
            <Label>{t("breakIns.overrideReason")}</Label>
            <Input className="h-8 text-sm" value={breakInOverrideReason} onChange={(e) => setBreakInOverrideReason(e.target.value)} />
          </div>
        </div>
        <PermissionGate permission="plan.confirm">
          <Button className="mt-3" size="sm" variant="outline" onClick={() => void handleCreateBreakIn()} disabled={!breakInCommitmentId || saving}>
            <Siren className="mr-2 h-3.5 w-3.5" />
            {t("breakIns.create")}
          </Button>
        </PermissionGate>
        <div className="mt-3 space-y-1 text-xs">
          {breakIns.map((entry) => (
            <div key={entry.id} className="rounded-lg border border-surface-border p-2">
              #{entry.id} {entry.break_in_reason} - {entry.old_slot_start} {"->"} {entry.new_slot_start} (
              {entry.cost_impact_delta?.toFixed(2) ?? "0.00"})
            </div>
          ))}
          {breakIns.length === 0 && <p className="text-text-muted">{t("breakIns.empty")}</p>}
        </div>
          </CardContent>
        </Card>

        {loading && <p className="mt-4 text-xs text-text-muted">{t("state.loading")}</p>}
      </div>
    </div>
  );
}

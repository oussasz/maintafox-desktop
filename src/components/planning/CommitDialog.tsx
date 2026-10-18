import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { useAbsenceTypeLookup } from "@/components/personnel/use-personnel-lists";
import {
  AssigneeCapacityDetails,
  assigneeOptionSuffix,
} from "@/components/planning/AssigneeCapacityDetails";
import { ConflictReasonList } from "@/components/planning/ConflictReasonList";
import { FrozenWeekBreakInDialog } from "@/components/planning/FrozenWeekBreakInDialog";
import { OverrideReasonDialog } from "@/components/planning/OverrideReasonDialog";
import {
  PLANNING_PROPOSED_ITEM_ID,
  PLANNING_TIMELINE_MAX_ZOOM_MS,
  PLANNING_UNASSIGNED_GROUP_ID,
  type PlanningBoardScope,
  buildPlanningTimelineItems,
  buildPlanningTimelineLanes,
  buildPlanningWindowHighlights,
  candidateDisplayCode,
  candidateDisplayTitle,
  checkSlotCapacity,
  commitmentTimelineState,
  deriveCommitSlotLocals,
  fromDateTimeLocal,
  hoursBetweenLocals,
  isCapacityGateError,
  overridableCodes,
  isAssigneeOverridableError,
  msToLocalIsoDate,
  parseLaborCostRange,
  parseMs,
  previewLaborCost,
  sanitizeBudgetThresholdInput,
  slotCapacityRows,
  slotShiftHours,
  teamGroupId,
  timelineItemTooltip,
} from "@/components/planning/planning-model";
import { useAssigneeCapacity } from "@/components/planning/use-assignee-capacity";
import {
  ResourceTimeline,
  type ResourceTimelineGroup,
  type ResourceTimelineItem,
} from "@/components/timeline/ResourceTimeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DateTimeLocalField } from "@/components/ui/date-time-local-field";
import {
  Dialog,
  DialogContent,
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
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorText } from "@/lib/app-error";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import { getActiveRateCard, listTeamMembers } from "@/services/personnel-service";
import { extractIpcValidationDetails } from "@/utils/errors";
import type {
  CandidateConflictSummary,
  CreateScheduleCommitmentInput,
  PlanningGanttSnapshot,
  PlanningWindow,
  ScheduleCandidate,
  ScheduleCommitment,
  TeamMemberRow,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type CrewOption = { id: number; label: string };

type CommitDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  candidate: ScheduleCandidate | null;
  boardScope: PlanningBoardScope;
  teams: CrewOption[];
  /** Implied crew when boardScope is single-crew; null in All Crews mode. */
  teamId: number | null;
  conflictSummary?: CandidateConflictSummary | null;
  gantt: PlanningGanttSnapshot | null;
  commitments: ScheduleCommitment[];
  lockedWindows: PlanningWindow[];
  horizonStart: number;
  horizonEnd: number;
  visibleStart: number;
  visibleEnd: number;
  saving: boolean;
  onSubmit: (input: CreateScheduleCommitmentInput) => Promise<void>;
  /** Who may approve a break-in (frozen week); same list as the break-in dialog. */
  approvers: { id: number; label: string }[];
};

export function CommitDialog({
  open,
  onOpenChange,
  candidate,
  boardScope,
  teams,
  teamId,
  conflictSummary = null,
  gantt,
  commitments,
  lockedWindows,
  horizonStart,
  horizonEnd,
  visibleStart,
  visibleEnd,
  saving,
  onSubmit,
  approvers,
}: CommitDialogProps) {
  const { t } = useTranslation("planning");
  const absenceType = useAbsenceTypeLookup();
  const { formatCurrency } = useFormatters();
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [targetCrewId, setTargetCrewId] = useState<number | null>(null);
  const [personnelId, setPersonnelId] = useState<number | null>(null);
  const [budgetThreshold, setBudgetThreshold] = useState("");
  const [roster, setRoster] = useState<TeamMemberRow[]>([]);
  const [rosterLoading, setRosterLoading] = useState(false);
  const [rosterError, setRosterError] = useState<string | null>(null);
  const [laborRate, setLaborRate] = useState<number | null>(null);
  const [rateLoading, setRateLoading] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [overrideOpen, setOverrideOpen] = useState(false);
  const [pendingOverrideInput, setPendingOverrideInput] =
    useState<CreateScheduleCommitmentInput | null>(null);
  /** The assignee refusals the pending override would cover (shown before confirming). */
  const [pendingOverrideCodes, setPendingOverrideCodes] = useState<string[]>([]);
  /** A commit refused because its week is frozen, waiting for its break-in reason. */
  const [pendingFrozenInput, setPendingFrozenInput] =
    useState<CreateScheduleCommitmentInput | null>(null);
  const [frozenWeekStart, setFrozenWeekStart] = useState<string | null>(null);

  const requireTargetCrew = boardScope.kind === "all";
  const effectiveTeamId = requireTargetCrew ? targetCrewId : teamId;
  const laneMode = requireTargetCrew ? "crew" : "technician";

  const horizonMinDate = useMemo(() => msToLocalIsoDate(horizonStart), [horizonStart]);
  const horizonMaxDate = useMemo(() => msToLocalIsoDate(horizonEnd), [horizonEnd]);

  useEffect(() => {
    if (!candidate || !open) return;
    const slot = deriveCommitSlotLocals(candidate);
    setStart(slot.startLocal);
    setEnd(slot.endLocal);
    setPersonnelId(candidate.assigned_personnel_id);
    setTargetCrewId(
      requireTargetCrew
        ? candidate.assigned_team_id != null &&
          teams.some((c) => c.id === candidate.assigned_team_id)
          ? candidate.assigned_team_id
          : null
        : null,
    );
    setBudgetThreshold("");
    setFieldError(null);
    setFormError(null);
    setPendingOverrideInput(null);
  }, [candidate, open, requireTargetCrew, teams]);

  useEffect(() => {
    if (!open || effectiveTeamId == null) {
      setRoster([]);
      setRosterError(null);
      setRosterLoading(false);
      return;
    }
    let cancelled = false;
    setRosterLoading(true);
    setRosterError(null);
    void (async () => {
      try {
        const members = await listTeamMembers(effectiveTeamId);
        if (cancelled) return;
        setRoster(members);
      } catch (err) {
        if (cancelled) return;
        setRoster([]);
        setRosterError(appErrorText(err));
      } finally {
        if (!cancelled) setRosterLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, effectiveTeamId]);

  useEffect(() => {
    if (!open || personnelId == null) {
      setLaborRate(null);
      setRateLoading(false);
      return;
    }
    let cancelled = false;
    setRateLoading(true);
    void (async () => {
      try {
        const card = await getActiveRateCard(personnelId);
        if (cancelled) return;
        setLaborRate(card != null && Number.isFinite(card.labor_rate) ? card.labor_rate : null);
      } catch {
        if (cancelled) return;
        setLaborRate(null);
      } finally {
        if (!cancelled) setRateLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, personnelId]);

  const rosterIds = useMemo(() => new Set(roster.map((m) => m.personnel_id)), [roster]);

  /** Each crew member over the chosen slot: shift and hours left per day, refusals. */
  const assigneeCapacity = useAssigneeCapacity({
    enabled: open,
    teamId: effectiveTeamId,
    startIso: start.trim() ? fromDateTimeLocal(start) : "",
    endIso: end.trim() ? fromDateTimeLocal(end) : "",
    candidateId: candidate?.id ?? null,
  });

  const assigneeOptions = useMemo(
    () =>
      roster.map((m) => {
        const name = m.employee_code
          ? `${formatPersonLabel(m.full_name)} (${m.employee_code})`
          : formatPersonLabel(m.full_name);
        const cap = assigneeCapacity.get(m.personnel_id);
        return {
          id: m.personnel_id,
          label: cap ? `${name} — ${assigneeOptionSuffix(cap, t)}` : name,
        };
      }),
    [roster, assigneeCapacity, t],
  );
  const selectedCapacity = personnelId != null ? assigneeCapacity.get(personnelId) : undefined;

  // Drop assignee if no longer on crew (e.g. crew changed while dialog open).
  useEffect(() => {
    if (personnelId == null) return;
    if (rosterLoading) return;
    if (roster.length === 0) return;
    if (!rosterIds.has(personnelId)) {
      setPersonnelId(null);
    }
  }, [personnelId, rosterLoading, roster.length, rosterIds]);

  const costRange = parseLaborCostRange(candidate?.estimated_labor_cost_range_json);
  const code = candidate ? candidateDisplayCode(candidate) : "";
  const title = candidate ? candidateDisplayTitle(candidate) : "";
  const blockers = conflictSummary?.blocker_codes ?? [];
  const reasons = conflictSummary?.reasons ?? [];

  const durationHours = useMemo(() => hoursBetweenLocals(start, end), [start, end]);

  /** Labour hours the work books: duration × crew size (as the backend gate). None without both. */
  const labourHours = useMemo(() => {
    const h = candidate?.estimated_duration_hours;
    const crew = candidate?.planned_crew_size;
    return h != null && Number.isFinite(h) && h > 0 && crew != null && crew >= 1 ? h * crew : null;
  }, [candidate?.estimated_duration_hours, candidate?.planned_crew_size]);
  const notEstimated = candidate != null && labourHours == null;

  /** Every day of the slot: its share of the labour against the team's remaining hours. */
  const slotCapacity = useMemo(() => {
    if (effectiveTeamId == null || !start.trim() || !end.trim() || labourHours == null) return null;
    return checkSlotCapacity({
      capacity: gantt?.capacity,
      teamId: effectiveTeamId,
      startIso: fromDateTimeLocal(start),
      endIso: fromDateTimeLocal(end),
      labourHours,
    });
  }, [effectiveTeamId, start, end, labourHours, gantt?.capacity]);

  const capacityBlocked = slotCapacity?.overloaded != null;

  /**
   * Each person works the whole duration: warn (not block) when the slot holds less shift time
   * than that. Unknown when a day's shift windows are not known.
   */
  const shiftTimeShort = useMemo(() => {
    const duration = candidate?.estimated_duration_hours;
    if (effectiveTeamId == null || !start.trim() || !end.trim() || duration == null) return null;
    const startIso = fromDateTimeLocal(start);
    const endIso = fromDateTimeLocal(end);
    const shiftHours = slotShiftHours(
      startIso,
      endIso,
      slotCapacityRows(gantt?.capacity, effectiveTeamId, startIso, endIso),
    );
    return shiftHours != null && duration > shiftHours + 0.01 ? { duration, shiftHours } : null;
  }, [candidate?.estimated_duration_hours, effectiveTeamId, start, end, gantt?.capacity]);

  const isLocalDateAtCapacity = useMemo(() => {
    if (effectiveTeamId == null) return () => false;
    const team = effectiveTeamId;
    const rows = gantt?.capacity;
    // Only days known to be full from the load data (a working day with nothing left).
    return (localDate: string) => {
      const row = (rows ?? []).find((c) => c.team_id === team && c.work_date === localDate);
      if (!row || row.available_hours + row.overtime_hours <= 0) return false;
      return row.available_hours + row.overtime_hours - row.committed_hours <= 0;
    };
  }, [effectiveTeamId, gantt?.capacity]);

  const slotOutsideHorizon = useMemo(() => {
    if (!start.trim() && !end.trim()) return false;
    const check = (local: string) => {
      if (!local.trim()) return false;
      const ms = parseMs(fromDateTimeLocal(local));
      if (!ms) return false;
      return ms < horizonStart || ms > horizonEnd;
    };
    return check(start) || check(end);
  }, [start, end, horizonStart, horizonEnd]);

  const estimateDisplay = useMemo(() => {
    if (labourHours != null && personnelId != null && laborRate != null) {
      return {
        kind: "computed" as const,
        value: previewLaborCost(labourHours, laborRate),
      };
    }
    if (personnelId != null && rateLoading) {
      return { kind: "loading" as const };
    }
    if (personnelId != null && laborRate == null && !rateLoading) {
      if (costRange) {
        return { kind: "range" as const, min: costRange.min, max: costRange.max };
      }
      return { kind: "noRate" as const };
    }
    if (personnelId != null && laborRate != null && labourHours == null) {
      return { kind: "needSlot" as const };
    }
    if (costRange) {
      return { kind: "range" as const, min: costRange.min, max: costRange.max };
    }
    if (personnelId == null) {
      return { kind: "needAssignee" as const };
    }
    return { kind: "loading" as const };
  }, [labourHours, personnelId, laborRate, costRange, rateLoading]);

  const assigneeDisabled = effectiveTeamId == null || rosterLoading || Boolean(rosterError);
  const assigneeHelper = (() => {
    if (effectiveTeamId == null) {
      return requireTargetCrew
        ? t("commitment.assigneeDisabledNeedTargetCrew")
        : t("commitment.assigneeDisabledNoCrew");
    }
    if (rosterLoading) return t("commitment.rosterLoading");
    if (rosterError) return rosterError;
    if (roster.length === 0) return t("commitment.rosterEmptyHint");
    return null;
  })();

  const previewGroups = useMemo<ResourceTimelineGroup[]>(() => {
    if (laneMode === "crew") {
      const lanes = gantt?.crew_lanes ?? [];
      const rows: ResourceTimelineGroup[] = [
        {
          id: PLANNING_UNASSIGNED_GROUP_ID,
          title: (
            <div className="flex flex-col leading-tight">
              <span className="text-xs font-medium text-text-primary">
                {t("board.unassignedLane")}
              </span>
            </div>
          ),
        },
      ];
      for (const lane of lanes) {
        rows.push({
          id: teamGroupId(lane.team_id),
          title: (
            <div className="flex flex-col leading-tight">
              <span className="truncate text-xs font-medium text-text-primary">
                {lane.team_name}
              </span>
            </div>
          ),
        });
      }
      return rows;
    }
    return buildPlanningTimelineLanes(gantt).map((lane) => ({
      id: lane.id,
      title: (
        <div className="flex flex-col leading-tight">
          <span className="truncate text-xs font-medium text-text-primary">
            {lane.personnelId == null
              ? t("board.unassignedLane")
              : formatPersonLabel(lane.fullName)}
          </span>
        </div>
      ),
    }));
  }, [gantt, laneMode, t]);

  const previewItems = useMemo<ResourceTimelineItem[]>(() => {
    let base: ResourceTimelineItem[];
    if (laneMode === "crew") {
      base = [];
      for (const commitment of commitments) {
        const startMs = parseMs(commitment.committed_start);
        const endMs = parseMs(commitment.committed_end);
        if (!startMs || !endMs || endMs <= startMs) continue;
        const title =
          commitment.source_code?.trim() ||
          commitment.source_title?.trim() ||
          commitment.source_type;
        base.push({
          id: `cmt-${commitment.id}`,
          group: teamGroupId(commitment.assigned_team_id),
          title,
          start_time: startMs,
          end_time: endMs,
          state: commitmentTimelineState(commitment),
          canMove: false,
        });
      }
    } else {
      base = buildPlanningTimelineItems({
        commitments,
        gantt,
        canDrag: false,
        absenceType,
      }).map((row) => ({
        id: row.id,
        group: row.groupId,
        title: row.title,
        start_time: row.startMs,
        end_time: row.endMs,
        state: row.state,
        canMove: false,
        tooltip: timelineItemTooltip(row, t),
      }));
    }

    if (!start.trim() || !end.trim()) return base;
    const startMs = parseMs(fromDateTimeLocal(start));
    const endMs = parseMs(fromDateTimeLocal(end));
    if (!startMs || !endMs || endMs <= startMs) return base;

    const proposedGroup =
      laneMode === "crew"
        ? effectiveTeamId != null
          ? teamGroupId(effectiveTeamId)
          : PLANNING_UNASSIGNED_GROUP_ID
        : personnelId != null
          ? String(personnelId)
          : PLANNING_UNASSIGNED_GROUP_ID;

    base.push({
      id: PLANNING_PROPOSED_ITEM_ID,
      group: proposedGroup,
      title: t("commitment.previewBar", {
        code: code || t("commitment.selectedCandidate"),
      }),
      start_time: startMs,
      end_time: endMs,
      state: "partial",
      canMove: false,
    });
    return base;
  }, [
    commitments,
    gantt,
    start,
    end,
    personnelId,
    code,
    t,
    laneMode,
    effectiveTeamId,
    absenceType,
  ]);

  const highlights = useMemo(() => buildPlanningWindowHighlights(lockedWindows), [lockedWindows]);

  function validate(): CreateScheduleCommitmentInput | null {
    if (!candidate) {
      setFieldError(t("commitment.errors.noCandidate"));
      return null;
    }
    if (effectiveTeamId == null) {
      setFieldError(
        requireTargetCrew
          ? t("commitment.errors.targetCrewRequired")
          : t("commitment.errors.crewRequired"),
      );
      return null;
    }
    if (!start.trim() || !end.trim()) {
      setFieldError(t("commitment.errors.slotRequired"));
      return null;
    }
    const committedStart = fromDateTimeLocal(start);
    const committedEnd = fromDateTimeLocal(end);
    const startMs = parseMs(committedStart);
    const endMs = parseMs(committedEnd);
    if (!startMs || !endMs) {
      setFieldError(t("commitment.errors.slotInvalid"));
      return null;
    }
    if (endMs <= startMs) {
      setFieldError(t("commitment.errors.endAfterStart"));
      return null;
    }
    if (
      startMs < horizonStart ||
      startMs > horizonEnd ||
      endMs < horizonStart ||
      endMs > horizonEnd
    ) {
      setFieldError(t("commitment.errors.outsideHorizon"));
      return null;
    }
    if (personnelId != null && !rosterIds.has(personnelId)) {
      setFieldError(t("commitment.errors.assigneeNotOnCrew"));
      return null;
    }
    if (labourHours == null) {
      setFieldError(t("commitment.notEstimated"));
      return null;
    }
    const check = checkSlotCapacity({
      capacity: gantt?.capacity,
      teamId: effectiveTeamId,
      startIso: committedStart,
      endIso: committedEnd,
      labourHours,
    });
    if (check.overloaded) {
      setFieldError(
        t("commitment.errors.capacityExceededDay", {
          day: check.overloaded.day,
          share: check.overloaded.shareHours.toFixed(1),
          remaining: check.overloaded.remainingHours.toFixed(1),
        }),
      );
      return null;
    }
    const budgetRaw = budgetThreshold.trim();
    let budget: number | null = null;
    if (budgetRaw) {
      const n = Number(budgetRaw);
      if (!Number.isFinite(n) || n < 0) {
        setFieldError(t("commitment.errors.budgetInvalid"));
        return null;
      }
      budget = n;
    }

    setFieldError(null);
    return {
      schedule_candidate_id: candidate.id,
      expected_candidate_row_version: candidate.row_version,
      committed_start: committedStart,
      committed_end: committedEnd,
      assigned_team_id: effectiveTeamId,
      assigned_personnel_id: personnelId,
      allow_double_booking_override: false,
      budget_threshold: budget,
    };
  }

  async function submit(input: CreateScheduleCommitmentInput) {
    setFormError(null);
    try {
      await onSubmit(input);
      onOpenChange(false);
    } catch (err) {
      const message = appErrorText(err);
      const details = extractIpcValidationDetails(err);
      const codes = details.map((d) => d.code);
      if (isCapacityGateError(codes)) {
        const overloaded = slotCapacity?.overloaded;
        setFormError(
          overloaded
            ? t("commitment.errors.capacityExceededDay", {
                day: overloaded.day,
                share: overloaded.shareHours.toFixed(1),
                remaining: overloaded.remainingHours.toFixed(1),
              })
            : message,
        );
      } else {
        setFormError(message);
      }
      const frozen = details.find((d) => d.code === "PLANNING_WEEK_FROZEN");
      if (frozen && !input.frozen_week_break_in) {
        setFrozenWeekStart(frozen.params?.["week_start"] ?? null);
        setPendingFrozenInput(input);
      }
      if (isAssigneeOverridableError(codes)) {
        setPendingOverrideInput(input);
        setPendingOverrideCodes(overridableCodes(codes));
      }
    }
  }

  async function handleSubmit() {
    const input = validate();
    if (!input) return;
    await submit(input);
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent
          className="flex max-h-[85vh] max-w-5xl flex-col gap-0 overflow-hidden p-0"
          onPointerDownOutside={(e) => e.preventDefault()}
          onFocusOutside={(e) => e.preventDefault()}
          onInteractOutside={(e) => e.preventDefault()}
        >
          <DialogHeader className="shrink-0 border-b border-surface-border px-6 py-4">
            <DialogTitle>{t("commitment.formTitle")}</DialogTitle>
          </DialogHeader>

          <div className="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[minmax(280px,38%)_1fr]">
            <div className="flex min-h-0 flex-col gap-3 overflow-y-auto border-b border-surface-border p-4 md:border-b-0 md:border-r">
              <div>
                <Label>{t("commitment.selectedCandidate")}</Label>
                <div className="mt-1 flex flex-wrap items-center gap-2">
                  <Input
                    className="h-8 flex-1 text-sm font-mono"
                    readOnly
                    value={formatOrDash(code)}
                  />
                  {candidate ? (
                    <Badge variant="outline" className="text-[10px]">
                      {t(`filters.sourceTypes.${candidate.source_type}`, {
                        defaultValue: candidate.source_type,
                      })}
                    </Badge>
                  ) : null}
                </div>
                {title ? <p className="mt-1 text-xs text-text-muted">{title}</p> : null}
              </div>

              {candidate ? (
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="secondary" className="text-[10px]">
                    {t("backlog.readinessScore")}: {candidate.readiness_score.toFixed(1)}
                  </Badge>
                  {candidate.priority_label ? (
                    <Badge variant="outline" className="text-[10px]">
                      {candidate.priority_label}
                    </Badge>
                  ) : null}
                </div>
              ) : null}

              {blockers.length > 0 ? (
                <div className="flex flex-wrap gap-1">
                  {[...new Set(blockers)].slice(0, 8).map((codeKey) => (
                    <Badge key={codeKey} variant="destructive" className="text-[10px] font-normal">
                      {t(`blockers.${codeKey}`, { defaultValue: codeKey })}
                    </Badge>
                  ))}
                </div>
              ) : null}
              <ConflictReasonList reasons={reasons} />

              <div className="flex flex-col gap-3">
                <DateTimeLocalField
                  id="commit-start"
                  label={t("commitment.start")}
                  value={start}
                  {...(horizonMinDate ? { minDate: horizonMinDate } : {})}
                  {...(horizonMaxDate ? { maxDate: horizonMaxDate } : {})}
                  isDateDisabled={isLocalDateAtCapacity}
                  onChange={(v) => {
                    setStart(v);
                    setFieldError(null);
                    setFormError(null);
                  }}
                />
                <DateTimeLocalField
                  id="commit-end"
                  label={t("commitment.end")}
                  value={end}
                  {...(horizonMinDate ? { minDate: horizonMinDate } : {})}
                  {...(horizonMaxDate ? { maxDate: horizonMaxDate } : {})}
                  isDateDisabled={isLocalDateAtCapacity}
                  onChange={(v) => {
                    setEnd(v);
                    setFieldError(null);
                    setFormError(null);
                  }}
                />
                <p className="text-[11px] text-text-muted">{t("commitment.horizonHint")}</p>
                {notEstimated ? (
                  <p className="text-[11px] text-tone-attention-fg" role="alert">
                    {t("commitment.notEstimated")}
                  </p>
                ) : labourHours != null ? (
                  <p className="text-[11px] text-text-muted">
                    {t("commitment.labourHours", {
                      hours: labourHours.toFixed(1),
                      duration: candidate?.estimated_duration_hours?.toFixed(1),
                      crew: candidate?.planned_crew_size,
                    })}
                  </p>
                ) : null}
                {slotCapacity && slotCapacity.days.length > 0 ? (
                  <ul className="space-y-0.5 text-[11px] text-text-muted">
                    {slotCapacity.days.map((d) => (
                      <li
                        key={d.day}
                        className={
                          slotCapacity.overloaded?.day === d.day
                            ? "text-tone-attention-fg"
                            : undefined
                        }
                      >
                        {t("commitment.capacityByDay", {
                          day: d.day,
                          share: d.shareHours.toFixed(1),
                          remaining: d.remainingHours.toFixed(1),
                          available: (d.availableHours + d.overtimeHours).toFixed(1),
                        })}
                        {d.basis && d.basis !== "roster"
                          ? ` · ${t(`commitment.capacityBasis.${d.basis}`, { defaultValue: d.basis })}`
                          : null}
                      </li>
                    ))}
                  </ul>
                ) : null}
                {slotCapacity?.noSchedule ? (
                  <p className="text-[11px] text-tone-attention-fg" role="alert">
                    {t("commitment.noTeamCapacity")}{" "}
                    <Link
                      to={`/personnel?tab=teams&team=${effectiveTeamId ?? ""}`}
                      className="underline"
                    >
                      {t("commitment.setUpTeamCapacity")}
                    </Link>
                  </p>
                ) : null}
                {shiftTimeShort ? (
                  <p className="text-[11px] text-tone-attention-fg" role="status">
                    {t("commitment.shiftTimeShort", {
                      duration: shiftTimeShort.duration.toFixed(1),
                      shift: shiftTimeShort.shiftHours.toFixed(1),
                    })}
                  </p>
                ) : null}
                {capacityBlocked && slotCapacity?.overloaded ? (
                  <p className="text-[11px] text-tone-attention-fg" role="alert">
                    {t("commitment.errors.capacityExceededDay", {
                      day: slotCapacity.overloaded.day,
                      share: slotCapacity.overloaded.shareHours.toFixed(1),
                      remaining: slotCapacity.overloaded.remainingHours.toFixed(1),
                    })}
                  </p>
                ) : null}
                {slotOutsideHorizon ? (
                  <p className="text-[11px] text-text-warning">
                    {t("commitment.horizonPrefillWarning")}
                  </p>
                ) : null}
              </div>

              {requireTargetCrew ? (
                <div>
                  <Label htmlFor="commit-target-crew">{t("commitment.targetCrew")}</Label>
                  <Select
                    value={targetCrewId != null ? String(targetCrewId) : ""}
                    onValueChange={(v) => {
                      if (!v) return;
                      setTargetCrewId(Number(v));
                      setPersonnelId(null);
                      setFieldError(null);
                      setFormError(null);
                    }}
                  >
                    <SelectTrigger id="commit-target-crew" className="mt-1 h-8 text-sm">
                      <SelectValue placeholder={t("commitment.targetCrewPlaceholder")} />
                    </SelectTrigger>
                    <SelectContent position="popper" className="z-[200]">
                      {teams.map((crew) => (
                        <SelectItem key={crew.id} value={String(crew.id)}>
                          {crew.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="mt-1 text-[11px] text-text-muted">
                    {t("commitment.targetCrewHint")}
                  </p>
                </div>
              ) : null}

              <div>
                <Label>{t("commitment.assignee")}</Label>
                <Select
                  value={personnelId != null ? String(personnelId) : "__unassigned__"}
                  onValueChange={(v) => {
                    setPersonnelId(v === "__unassigned__" ? null : Number(v));
                    setFieldError(null);
                    setFormError(null);
                  }}
                  disabled={assigneeDisabled}
                >
                  <SelectTrigger className="h-8 text-sm">
                    <SelectValue placeholder={t("commitment.assignee")} />
                  </SelectTrigger>
                  <SelectContent position="popper" className="z-[200]">
                    <SelectItem value="__unassigned__">{t("commitment.unassigned")}</SelectItem>
                    {assigneeOptions.map((person) => (
                      <SelectItem key={person.id} value={String(person.id)}>
                        {person.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <AssigneeCapacityDetails
                  capacity={selectedCapacity}
                  crewSize={candidate?.planned_crew_size ?? null}
                  assigned={personnelId != null}
                />
                {assigneeHelper ? (
                  <p className={cnHelper(rosterError != null)}>{assigneeHelper}</p>
                ) : (
                  <p className="mt-1 text-[11px] text-text-muted">{t("commitment.assigneeHint")}</p>
                )}
              </div>

              <div className="space-y-2 rounded border border-surface-border bg-surface-1 px-2.5 py-2">
                <div>
                  <Label>{t("commitment.estimatedLaborCost")}</Label>
                  <p className="mt-0.5 text-sm font-medium text-text-primary">
                    {estimateDisplay.kind === "computed"
                      ? t("commitment.estimatedLaborCostValue", {
                          value: formatCurrency(estimateDisplay.value),
                        })
                      : estimateDisplay.kind === "range"
                        ? t("commitment.costRange", {
                            min: formatCurrency(estimateDisplay.min),
                            max: formatCurrency(estimateDisplay.max),
                          })
                        : estimateDisplay.kind === "noRate"
                          ? t("commitment.estimatedLaborCostNoRate")
                          : estimateDisplay.kind === "needSlot"
                            ? t("commitment.estimatedLaborCostNeedSlot")
                            : estimateDisplay.kind === "needAssignee"
                              ? t("commitment.estimatedLaborCostNeedAssignee")
                              : t("commitment.estimatedLaborCostLoading")}
                  </p>
                  {durationHours != null ? (
                    <p className="mt-0.5 text-[11px] text-text-muted">
                      {t("commitment.durationHours", { hours: durationHours.toFixed(2) })}
                    </p>
                  ) : null}
                  <p className="mt-0.5 text-[11px] text-text-muted">
                    {t("commitment.estimatedLaborCostHint")}
                  </p>
                </div>

                <div>
                  <Label htmlFor="commit-budget-threshold">{t("commitment.budgetThreshold")}</Label>
                  <Input
                    id="commit-budget-threshold"
                    className="mt-1 h-8 text-sm"
                    inputMode="decimal"
                    value={budgetThreshold}
                    onChange={(e) => {
                      setBudgetThreshold(sanitizeBudgetThresholdInput(e.target.value));
                      setFieldError(null);
                    }}
                    placeholder={t("commitment.budgetThresholdPlaceholder")}
                  />
                  <p className="mt-1 text-[11px] text-text-muted">
                    {t("commitment.budgetThresholdHint")}
                  </p>
                </div>
              </div>

              {fieldError ? (
                <p className="rounded border border-destructive/30 bg-destructive/5 px-2 py-1.5 text-xs text-destructive">
                  {fieldError}
                </p>
              ) : null}

              {formError ? (
                <div className="space-y-2 rounded border border-destructive/30 bg-destructive/5 px-2 py-1.5">
                  <p className="text-xs text-destructive">{formError}</p>
                  {pendingOverrideInput ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="h-7 text-xs"
                      onClick={() => setOverrideOpen(true)}
                    >
                      {t("commitment.overrideDoubleBooking")}
                    </Button>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="flex min-h-[280px] flex-col gap-2 p-3 md:min-h-0">
              <p className="px-1 text-[10px] font-medium uppercase tracking-wide text-text-muted">
                {t("commitment.previewTitle")}
              </p>
              {requireTargetCrew && targetCrewId == null ? (
                <div className="flex flex-1 items-center justify-center rounded-lg border border-dashed border-surface-border px-4 text-center text-xs text-text-muted">
                  {t("commitment.previewNeedsTargetCrew")}
                </div>
              ) : !requireTargetCrew && teamId == null ? (
                <div className="flex flex-1 items-center justify-center rounded-lg border border-dashed border-surface-border px-4 text-center text-xs text-text-muted">
                  {t("commitment.previewNeedsCrew")}
                </div>
              ) : (
                <ResourceTimeline
                  groups={previewGroups}
                  items={previewItems}
                  visibleTimeStart={visibleStart || horizonStart}
                  visibleTimeEnd={visibleEnd || horizonEnd}
                  horizonStart={horizonStart}
                  horizonEnd={horizonEnd}
                  timeRangeHighlights={highlights}
                  selectedItemIds={[PLANNING_PROPOSED_ITEM_ID]}
                  emptyTitle={t("commitment.previewEmpty")}
                  className="min-h-[260px] flex-1"
                  showZoomControls
                  maxZoom={PLANNING_TIMELINE_MAX_ZOOM_MS}
                  sidebarWidth={140}
                  lineHeight={36}
                />
              )}
            </div>
          </div>

          <DialogFooter className="shrink-0 border-t border-surface-border px-6 py-3">
            <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
              {t("dialogs.cancel")}
            </Button>
            <PermissionGate permission={P.PLAN_CONFIRM}>
              <Button
                type="button"
                size="sm"
                disabled={
                  !candidate ||
                  effectiveTeamId == null ||
                  saving ||
                  rosterLoading ||
                  notEstimated ||
                  capacityBlocked ||
                  slotOutsideHorizon
                }
                onClick={() => void handleSubmit()}
              >
                {t("actions.commit")}
              </Button>
            </PermissionGate>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <FrozenWeekBreakInDialog
        open={pendingFrozenInput != null}
        onOpenChange={(next) => {
          if (!next) setPendingFrozenInput(null);
        }}
        weekStart={frozenWeekStart}
        approvers={approvers}
        onConfirm={(breakIn) => {
          const pending = pendingFrozenInput;
          if (!pending) return;
          setPendingFrozenInput(null);
          void submit({ ...pending, frozen_week_break_in: breakIn });
        }}
      />
      <OverrideReasonDialog
        open={overrideOpen}
        onOpenChange={setOverrideOpen}
        issues={pendingOverrideCodes}
        onConfirm={(reason) => {
          const pending = pendingOverrideInput;
          if (!pending) return;
          setPendingOverrideInput(null);
          void submit({
            ...pending,
            allow_double_booking_override: true,
            override_reason: reason,
            // Only the refusals shown in the dialog are overridden.
            acknowledged_assignee_issues: pendingOverrideCodes,
          });
        }}
      >
        <AssigneeCapacityDetails
          capacity={selectedCapacity}
          crewSize={candidate?.planned_crew_size ?? null}
          assigned={personnelId != null}
        />
      </OverrideReasonDialog>
    </>
  );
}

function cnHelper(isError: boolean): string {
  return isError ? "mt-1 text-[11px] text-destructive" : "mt-1 text-[11px] text-text-muted";
}

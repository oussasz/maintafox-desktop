import {
  Bell,
  CalendarDays,
  CalendarRange,
  ChevronDown,
  Download,
  GanttChart,
  Lock,
  RefreshCw,
  Siren,
  SquareStack,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { BreakInDialog } from "@/components/planning/BreakInDialog";
import { CommitDialog } from "@/components/planning/CommitDialog";
import { CommitmentInspectorDialog } from "@/components/planning/CommitmentInspectorDialog";
import { FreezeConfirmDialog } from "@/components/planning/FreezeConfirmDialog";
import { OverrideReasonDialog } from "@/components/planning/OverrideReasonDialog";
import { PlanningBacklogRail } from "@/components/planning/PlanningBacklogRail";
import { PlanningBoard } from "@/components/planning/PlanningBoard";
import { PlanningCalendarView } from "@/components/planning/PlanningCalendarView";
import { PlanningWindowDetailDialog } from "@/components/planning/PlanningWindowDetailDialog";
import { ProductionWindowDialog } from "@/components/planning/ProductionWindowDialog";
import {
  addDaysIso,
  candidateDisplayCode,
  candidateDisplayTitle,
  clampVisibleToHorizon,
  filterCommitmentsForBoardScope,
  localDateEndIso,
  localDateEndMs,
  localDateStartIso,
  localDateStartMs,
  parseMs,
  PLANNING_DEFAULT_HORIZON_DAYS,
  PLANNING_HORIZON_PRESETS,
  PLANNING_TIMELINE_MAX_ZOOM_MS,
  readStoredBoardScope,
  readStoredPageView,
  toLocalIsoDate,
  writeStoredBoardScope,
  writeStoredPageView,
  type PlanningBoardScope,
  type PlanningPageView,
} from "@/components/planning/planning-model";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { formatPersonLabel } from "@/lib/display";
import { listActiveTeams, listPersonnel } from "@/services/personnel-service";
import { listUsers } from "@/services/rbac-service";
import { useSessionStore } from "@/store/session-store";
import { usePlanningStore } from "@/stores/planning-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  CreateScheduleBreakInInput,
  CreateScheduleCommitmentInput,
  CreatePlanningWindowInput,
  PlanningWindow,
  RescheduleCommitmentInput,
  ScheduleCandidate,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type CrewOption = { id: number; label: string };
type PersonnelOption = { id: number; label: string };
type ApproverOption = { id: number; label: string };

type PendingReschedule = {
  input: RescheduleCommitmentInput;
  needsOverride: boolean;
};

export function PlanningPage() {
  const { t } = useTranslation("planning");
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const { can } = usePermissions();

  const backlog = usePlanningStore((s) => s.backlog);
  const commitments = usePlanningStore((s) => s.commitments);
  const planningWindows = usePlanningStore((s) => s.planningWindows);
  const breakIns = usePlanningStore((s) => s.breakIns);
  const capacityLoad = usePlanningStore((s) => s.capacityLoad);
  const ganttSnapshot = usePlanningStore((s) => s.ganttSnapshot);
  const changeLog = usePlanningStore((s) => s.changeLog);
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
  const loadChangeLog = usePlanningStore((s) => s.loadChangeLog);
  const createCommitment = usePlanningStore((s) => s.createCommitment);
  const rescheduleCommitment = usePlanningStore((s) => s.rescheduleCommitment);
  const createPlanningWindow = usePlanningStore((s) => s.createPlanningWindow);
  const createBreakIn = usePlanningStore((s) => s.createBreakIn);
  const freezeSchedule = usePlanningStore((s) => s.freezeSchedule);
  const notifyTeams = usePlanningStore((s) => s.notifyTeams);
  const exportGanttPdf = usePlanningStore((s) => s.exportGanttPdf);

  const sessionUserId = useSessionStore((s) => s.info?.user_id ?? null);

  const [teams, setTeams] = useState<CrewOption[]>([]);
  const [personnel, setPersonnel] = useState<PersonnelOption[]>([]);
  const [approvers, setApprovers] = useState<ApproverOption[]>([]);
  const [pageError, setPageError] = useState<string | null>(null);

  const [boardScope, setBoardScope] = useState<PlanningBoardScope>({ kind: "all" });
  const teamId = boardScope.kind === "crew" ? boardScope.teamId : null;
  const [pageView, setPageView] = useState<PlanningPageView>(() =>
    readStoredPageView(useSessionStore.getState().info?.user_id ?? null),
  );
  const [calendarSessionKey, setCalendarSessionKey] = useState(0);
  const [searchInput, setSearchInput] = useState("");
  const [readinessFilter, setReadinessFilter] = useState<string | null>(null);
  const [sourceTypeFilter, setSourceTypeFilter] = useState<string | null>(null);

  const [dateFrom, setDateFrom] = useState(() => toLocalIsoDate());
  const [dateTo, setDateTo] = useState(() =>
    addDaysIso(toLocalIsoDate(), PLANNING_DEFAULT_HORIZON_DAYS - 1),
  );
  const horizonStart = useMemo(() => localDateStartMs(dateFrom), [dateFrom]);
  const horizonEnd = useMemo(() => localDateEndMs(dateTo), [dateTo]);
  const [visibleStart, setVisibleStart] = useState(() => {
    const from = toLocalIsoDate();
    const to = addDaysIso(from, PLANNING_DEFAULT_HORIZON_DAYS - 1);
    return clampVisibleToHorizon(
      localDateStartMs(from),
      localDateEndMs(to),
      PLANNING_TIMELINE_MAX_ZOOM_MS,
    ).start;
  });
  const [visibleEnd, setVisibleEnd] = useState(() => {
    const from = toLocalIsoDate();
    const to = addDaysIso(from, PLANNING_DEFAULT_HORIZON_DAYS - 1);
    return clampVisibleToHorizon(
      localDateStartMs(from),
      localDateEndMs(to),
      PLANNING_TIMELINE_MAX_ZOOM_MS,
    ).end;
  });

  const [selectedCandidate, setSelectedCandidate] = useState<ScheduleCandidate | null>(null);
  const [selectedCommitmentId, setSelectedCommitmentId] = useState<number | null>(null);
  const [selectedWindow, setSelectedWindow] = useState<PlanningWindow | null>(null);

  const [commitOpen, setCommitOpen] = useState(false);
  const [windowOpen, setWindowOpen] = useState(false);
  const [windowDetailOpen, setWindowDetailOpen] = useState(false);
  const [breakInOpen, setBreakInOpen] = useState(false);
  const [breakInCommitmentId, setBreakInCommitmentId] = useState<number | null>(null);
  const [freezeOpen, setFreezeOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [overrideOpen, setOverrideOpen] = useState(false);
  const pendingRescheduleRef = useRef<PendingReschedule | null>(null);

  // Board IPC period — derived strictly from DU/AU, not from pan/zoom viewport.
  const periodStartIso = useMemo(() => localDateStartIso(dateFrom), [dateFrom]);
  const periodEndIso = useMemo(() => localDateEndIso(dateTo), [dateTo]);

  useEffect(() => {
    const next = clampVisibleToHorizon(horizonStart, horizonEnd, PLANNING_TIMELINE_MAX_ZOOM_MS);
    setVisibleStart(next.start);
    setVisibleEnd(next.end);
  }, [horizonStart, horizonEnd]);

  useEffect(() => {
    void loadBacklog();
  }, [loadBacklog]);

  /** Deep-link from WO Plan tab: `/planning?woId=<id>` */
  const deepLinkHandledRef = useRef<string | null>(null);
  /** OT id from a `?woId=` link that is not in the planning backlog (shown as a notice). */
  const [deepLinkMissWoId, setDeepLinkMissWoId] = useState<number | null>(null);
  useEffect(() => {
    const raw = searchParams.get("woId");
    if (!raw) {
      deepLinkHandledRef.current = null;
      return;
    }
    // Wait for the backlog to load (an empty backlog is a loaded backlog).
    if (!backlog || loading) return;
    if (deepLinkHandledRef.current === raw) return;
    deepLinkHandledRef.current = raw;

    const woId = Number(raw);
    const validId = Number.isFinite(woId) && woId > 0;
    const match = validId
      ? backlog.candidates.find((c) => c.source_type === "work_order" && c.source_id === woId)
      : undefined;
    if (!match) {
      // -1: malformed link (no OT to offer); otherwise offer to open the OT itself.
      setDeepLinkMissWoId(validId ? woId : -1);
      const cleared = new URLSearchParams(searchParams);
      cleared.delete("woId");
      setSearchParams(cleared, { replace: true });
      return;
    }
    setDeepLinkMissWoId(null);

    setSelectedCandidate(match);
    if (match.readiness_status === "ready" && can(P.PLAN_CONFIRM)) {
      setCommitOpen(true);
    }

    const next = new URLSearchParams(searchParams);
    next.delete("woId");
    setSearchParams(next, { replace: true });
  }, [backlog, loading, searchParams, setSearchParams, can]);

  useEffect(() => {
    void (async () => {
      try {
        const [crewRows, peoplePage, users] = await Promise.all([
          listActiveTeams({}),
          listPersonnel({ limit: 250, offset: 0 }),
          listUsers({}),
        ]);
        const crewOptions = crewRows.map((row) => ({
          id: row.id,
          label: `${row.name} (${row.code})`,
        }));
        setTeams(crewOptions);
        setPersonnel(
          peoplePage.items.map((row) => ({
            id: row.id,
            label: row.employee_code ? `${row.full_name} (${row.employee_code})` : row.full_name,
          })),
        );
        setApprovers(
          users.map((user) => ({
            id: user.id,
            label: formatPersonLabel(user.display_name, user.username),
          })),
        );

        const crewIds = new Set(crewOptions.map((c) => c.id));
        const userId = useSessionStore.getState().info?.user_id ?? null;
        const stored = readStoredBoardScope(userId);
        if (stored.kind === "crew" && crewIds.has(stored.teamId)) {
          setBoardScope(stored);
          return;
        }
        // Invalid stored crew (or All) → plant-wide All Crews.
        setBoardScope({ kind: "all" });
        writeStoredBoardScope(userId, { kind: "all" });
      } catch (err) {
        setPageError(toErrorMessage(err));
      }
    })();
  }, []);

  function selectBoardScope(next: PlanningBoardScope) {
    setBoardScope(next);
    writeStoredBoardScope(sessionUserId, next);
  }

  function selectPageView(next: PlanningPageView) {
    setPageView(next);
    writeStoredPageView(sessionUserId, next);
    if (next === "calendar") {
      setCalendarSessionKey((key) => key + 1);
    }
  }

  const handleCalendarVisibleRangeChange = useCallback((from: string, to: string) => {
    setDateFrom((prev) => (prev === from ? prev : from));
    setDateTo((prev) => (prev === to ? prev : to));
  }, []);

  const calendarCommitments = useMemo(
    () => filterCommitmentsForBoardScope(commitments, boardScope),
    [commitments, boardScope],
  );

  const calendarWindows = ganttSnapshot?.locked_windows ?? planningWindows;

  function handleOpenOnBoard(commitmentId: number) {
    const commitment = commitments.find((entry) => entry.id === commitmentId);
    selectPageView("gantt");
    setSelectedCommitmentId(commitmentId);
    setInspectorOpen(true);
    if (!commitment) return;
    const start = parseMs(commitment.committed_start);
    const end = parseMs(commitment.committed_end);
    if (!start || !end || end <= start) return;
    const span = Math.min(end - start, PLANNING_TIMELINE_MAX_ZOOM_MS);
    const focusStart = Math.max(horizonStart, Math.min(start, horizonEnd - span));
    const focusEnd = Math.min(horizonEnd, Math.max(end, focusStart + Math.min(span, 3_600_000)));
    if (focusEnd > focusStart) {
      setVisibleStart(focusStart);
      setVisibleEnd(focusEnd);
    }
  }

  const reloadBoard = useCallback(async () => {
    const scopeTeamId = boardScope.kind === "crew" ? boardScope.teamId : null;
    const boardTasks: Array<Promise<unknown>> = [
      loadPlanningWindows({ include_locked: true }),
      loadBreakIns({
        period_start: periodStartIso,
        period_end: periodEndIso,
      }),
      loadGantt({
        period_start: periodStartIso,
        period_end: periodEndIso,
        team_id: scopeTeamId,
      }),
    ];
    if (scopeTeamId != null) {
      boardTasks.push(
        loadCommitments({
          period_start: periodStartIso,
          period_end: periodEndIso,
          team_id: scopeTeamId,
        }),
        loadCapacityLoad(periodStartIso, periodEndIso, scopeTeamId),
      );
    }
    await Promise.all(boardTasks);
  }, [
    boardScope,
    periodStartIso,
    periodEndIso,
    loadCommitments,
    loadCapacityLoad,
    loadPlanningWindows,
    loadBreakIns,
    loadGantt,
  ]);

  useEffect(() => {
    void reloadBoard();
  }, [reloadBoard]);

  const readyCandidates = useMemo(
    () => backlog?.candidates.filter((c) => c.readiness_status === "ready") ?? [],
    [backlog],
  );
  const blockedCandidates = useMemo(
    () => backlog?.candidates.filter((c) => c.readiness_status !== "ready") ?? [],
    [backlog],
  );

  const filterCandidate = useCallback(
    (candidate: ScheduleCandidate) => {
      // Board crew (teamId) intentionally does NOT filter the backlog rail.
      // Unassigned + any-crew candidates stay visible so planners can commit
      // them onto the selected board crew. Team only scopes the timeline.
      if (sourceTypeFilter && candidate.source_type !== sourceTypeFilter) return false;
      if (readinessFilter && candidate.readiness_status !== readinessFilter) return false;
      const q = searchInput.trim().toLowerCase();
      if (!q) return true;
      const haystack = [
        candidateDisplayCode(candidate),
        candidateDisplayTitle(candidate),
        candidate.source_type,
        candidate.assigned_personnel_name ?? "",
        candidate.assigned_team_label ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    },
    [searchInput, sourceTypeFilter, readinessFilter],
  );

  const filteredReady = useMemo(
    () => readyCandidates.filter(filterCandidate),
    [readyCandidates, filterCandidate],
  );
  const filteredBlocked = useMemo(
    () => blockedCandidates.filter(filterCandidate),
    [blockedCandidates, filterCandidate],
  );

  const selectedCommitment = commitments.find((c) => c.id === selectedCommitmentId) ?? null;

  const sourceTypeOptions = useMemo(() => {
    const set = new Set<string>();
    for (const c of backlog?.candidates ?? []) set.add(c.source_type);
    return [...set].map((value) => ({
      value,
      label: t(`filters.sourceTypes.${value}`, { defaultValue: value }),
    }));
  }, [backlog?.candidates, t]);

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "readiness",
        kind: "select",
        label: t("filters.readiness"),
        value: readinessFilter,
        options: [
          { value: "ready", label: t("filters.readinessReady") },
          { value: "blocked", label: t("filters.readinessBlocked") },
        ],
        onChange: (v) => setReadinessFilter(v),
      },
      {
        id: "sourceType",
        kind: "select",
        label: t("filters.sourceType"),
        value: sourceTypeFilter,
        options: sourceTypeOptions,
        onChange: (v) => setSourceTypeFilter(v),
      },
    ],
    [t, readinessFilter, sourceTypeFilter, sourceTypeOptions],
  );

  function resetFilters() {
    setSearchInput("");
    setReadinessFilter(null);
    setSourceTypeFilter(null);
  }

  async function handleCreateCommitment(input: CreateScheduleCommitmentInput) {
    setPageError(null);
    try {
      await createCommitment(input);
      await refreshBacklog();
      await reloadBoard();
    } catch (err) {
      setPageError(toErrorMessage(err));
      throw err;
    }
  }

  async function applyReschedule(input: RescheduleCommitmentInput) {
    setPageError(null);
    try {
      await rescheduleCommitment(input);
      await reloadBoard();
    } catch (err) {
      const text = toErrorMessage(err);
      if (text.toLowerCase().includes("double-booking") && !input.allow_double_booking_override) {
        pendingRescheduleRef.current = { input, needsOverride: true };
        setOverrideOpen(true);
        return;
      }
      setPageError(text);
    }
  }

  function handleBoardReschedule(
    commitmentId: number,
    newStartMs: number,
    personnelId: number | null,
    dropTeamId: number | null,
  ) {
    const commitment = commitments.find((entry) => entry.id === commitmentId);
    if (!commitment) return;
    const oldStart = Date.parse(commitment.committed_start);
    const oldEnd = Date.parse(commitment.committed_end);
    if (!oldStart || !oldEnd) return;
    const durationMs = oldEnd - oldStart;
    const nextStart = new Date(newStartMs);
    const nextEnd = new Date(newStartMs + durationMs);
    const nextTeamId =
      dropTeamId ?? (boardScope.kind === "crew" ? boardScope.teamId : commitment.assigned_team_id);
    void applyReschedule({
      commitment_id: commitment.id,
      expected_row_version: commitment.row_version,
      committed_start: nextStart.toISOString(),
      committed_end: nextEnd.toISOString(),
      assigned_team_id: nextTeamId,
      assigned_personnel_id:
        boardScope.kind === "all" ? commitment.assigned_personnel_id : personnelId,
      allow_double_booking_override: false,
      budget_threshold: commitment.budget_threshold,
    });
  }

  async function handleCreateWindow(input: CreatePlanningWindowInput) {
    setPageError(null);
    try {
      await createPlanningWindow(input);
      await reloadBoard();
    } catch (err) {
      setPageError(toErrorMessage(err));
      throw err;
    }
  }

  async function handleCreateBreakIn(input: CreateScheduleBreakInInput) {
    setPageError(null);
    try {
      await createBreakIn(input);
      await reloadBoard();
    } catch (err) {
      setPageError(toErrorMessage(err));
      throw err;
    }
  }

  async function handleFreeze(reason: string) {
    setPageError(null);
    try {
      await freezeSchedule({
        period_start: periodStartIso,
        period_end: periodEndIso,
        reason,
      });
      await reloadBoard();
    } catch (err) {
      setPageError(toErrorMessage(err));
      throw err;
    }
  }

  async function handleNotifyTeams() {
    setPageError(null);
    try {
      await notifyTeams({
        period_start: periodStartIso,
        period_end: periodEndIso,
        team_id: teamId,
        include_break_ins: true,
      });
    } catch (err) {
      setPageError(toErrorMessage(err));
    }
  }

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <CalendarRange className={mfLayout.moduleHeaderIcon} aria-hidden />
          <h1 className={mfLayout.moduleTitle}>{t("page.title", "Planning")}</h1>
          <Badge variant="secondary" className="text-xs">
            {backlog?.candidate_count ?? commitments.length}
          </Badge>
        </div>

        <div className={mfLayout.moduleHeaderActions}>
          <PermissionGate permission={P.PLAN_EDIT}>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void refreshBacklog()}
              disabled={saving}
              className="gap-1.5"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("actions.refreshBacklog")}
            </Button>
          </PermissionGate>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" variant="outline" className="gap-1.5">
                <Wrench className="h-3.5 w-3.5" />
                {t("actions.plannerTools")}
                <ChevronDown className="h-3.5 w-3.5 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              <PermissionGate permission={P.PLAN_WINDOWS}>
                <DropdownMenuItem onClick={() => setWindowOpen(true)}>
                  <SquareStack className="mr-2 h-3.5 w-3.5" />
                  {t("windows.title")}
                </DropdownMenuItem>
              </PermissionGate>
              <PermissionGate permission={P.PLAN_CONFIRM}>
                <DropdownMenuItem
                  disabled={commitments.length === 0 || saving}
                  title={commitments.length === 0 ? t("actions.requiresCommitments") : undefined}
                  onClick={() => setFreezeOpen(true)}
                >
                  <Lock className="mr-2 h-3.5 w-3.5" />
                  {t("actions.freezePeriod")}
                </DropdownMenuItem>
              </PermissionGate>
              <PermissionGate permission={P.PLAN_CONFIRM}>
                <DropdownMenuItem
                  disabled={commitments.length === 0 || saving}
                  title={commitments.length === 0 ? t("actions.requiresCommitments") : undefined}
                  onClick={() => setBreakInOpen(true)}
                >
                  <Siren className="mr-2 h-3.5 w-3.5" />
                  {t("breakIns.title")}
                </DropdownMenuItem>
              </PermissionGate>
              <PermissionGate permission={P.PLAN_CONFIRM}>
                <DropdownMenuItem
                  disabled={commitments.length === 0 || saving}
                  title={commitments.length === 0 ? t("actions.requiresCommitments") : undefined}
                  onClick={() => void handleNotifyTeams()}
                >
                  <Bell className="mr-2 h-3.5 w-3.5" />
                  {t("actions.notifyTeams")}
                </DropdownMenuItem>
              </PermissionGate>
              {commitments.length === 0 ? (
                <>
                  <DropdownMenuSeparator />
                  <div className="px-2 py-1.5 text-[11px] text-text-muted">
                    {t("actions.requiresCommitments")}
                  </div>
                </>
              ) : null}
            </DropdownMenuContent>
          </DropdownMenu>

          <div className={mfLayout.viewToggleGroup}>
            <Button
              variant={pageView === "gantt" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => selectPageView("gantt")}
              title={t("views.gantt")}
            >
              <GanttChart className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={pageView === "calendar" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => selectPageView("calendar")}
              title={t("views.calendar")}
            >
              <CalendarDays className="h-3.5 w-3.5" />
            </Button>
          </div>

          <Button
            size="sm"
            variant="outline"
            onClick={() => void exportGanttPdf(periodStartIso, periodEndIso, teamId)}
            className="gap-1.5"
          >
            <Download className="h-3.5 w-3.5" />
            {t("actions.exportPdf")}
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-2 border-b border-surface-border bg-surface-1/60 px-6 py-3">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-text-primary">
            {t("context.title")}
          </p>
          {!teamId ? (
            <p className="text-[11px] text-text-muted">{t("context.modeAllHint")}</p>
          ) : (
            <p className="text-[11px] text-text-muted">{t("context.modeCrewHint")}</p>
          )}
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div className="min-w-[220px] space-y-1">
            <Label htmlFor="planning-board-crew" className="text-[10px] uppercase tracking-wide">
              {t("filters.boardCrew")}
            </Label>
            <Select
              value={boardScope.kind === "all" ? "__all__" : String(boardScope.teamId)}
              onValueChange={(v) => {
                if (v === "__all__") {
                  selectBoardScope({ kind: "all" });
                  return;
                }
                const id = Number(v);
                if (Number.isFinite(id)) selectBoardScope({ kind: "crew", teamId: id });
              }}
            >
              <SelectTrigger id="planning-board-crew" className="h-8 w-full min-w-[220px] text-sm">
                <SelectValue placeholder={t("context.allCrews")} />
              </SelectTrigger>
              <SelectContent className="z-[200]">
                <SelectItem value="__all__">{t("context.allCrews")}</SelectItem>
                {teams.map((team) => (
                  <SelectItem key={team.id} value={String(team.id)}>
                    {team.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label htmlFor="planning-date-from" className="text-[10px] uppercase tracking-wide">
              {t("filters.periodStart")}
              <span className="ml-0.5 text-destructive" aria-hidden>
                *
              </span>
            </Label>
            <Input
              id="planning-date-from"
              className="h-8 w-[150px] text-sm"
              type="date"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor="planning-date-to" className="text-[10px] uppercase tracking-wide">
              {t("filters.periodEnd")}
              <span className="ml-0.5 text-destructive" aria-hidden>
                *
              </span>
            </Label>
            <Input
              id="planning-date-to"
              className="h-8 w-[150px] text-sm"
              type="date"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
            />
          </div>
          <div className="flex flex-wrap gap-1 pb-0.5">
            {PLANNING_HORIZON_PRESETS.map((days) => (
              <Button
                key={days}
                type="button"
                size="sm"
                variant="outline"
                className="h-8 text-xs"
                onClick={() => {
                  const from = toLocalIsoDate();
                  setDateFrom(from);
                  setDateTo(addDaysIso(from, days - 1));
                }}
              >
                {t("filters.horizonDays", { count: days })}
              </Button>
            ))}
          </div>
        </div>
      </div>

      <SmartFilterBar
        searchPlaceholder={t("filters.searchPlaceholder")}
        searchValue={searchInput}
        onSearchInputChange={setSearchInput}
        onSearchChange={setSearchInput}
        filters={filterDefs}
        resultCount={filteredReady.length + filteredBlocked.length}
        onReset={resetFilters}
      />

      {deepLinkMissWoId != null && (
        <div
          role="status"
          className="flex flex-wrap items-center justify-between gap-2 border-b border-surface-border bg-surface-1 px-6 py-2 text-sm text-text-primary"
        >
          <span>{t("deepLink.woNotInBacklog")}</span>
          <div className="flex items-center gap-2">
            {deepLinkMissWoId > 0 ? (
              <Button
                size="sm"
                variant="outline"
                className="h-9"
                onClick={() => void navigate(`/work-orders?openWo=${deepLinkMissWoId}`)}
              >
                {t("deepLink.openWo")}
              </Button>
            ) : null}
            <Button
              size="sm"
              variant="ghost"
              className="h-9"
              onClick={() => setDeepLinkMissWoId(null)}
            >
              {t("deepLink.dismiss")}
            </Button>
          </div>
        </div>
      )}

      {(pageError || storeError) && (
        <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive">
          {pageError ?? storeError}
        </div>
      )}

      <div className={mfLayout.moduleWorkspace}>
        <div className="flex min-h-0 flex-1 overflow-hidden">
          <PlanningBacklogRail
            ready={filteredReady}
            blocked={filteredBlocked}
            conflictSummary={backlog?.conflict_summary ?? []}
            selectedCandidateId={selectedCandidate?.id ?? null}
            onSelectReady={(candidate) => {
              setSelectedCandidate(candidate);
              setCommitOpen(true);
            }}
          />
          <div className="flex min-h-0 min-w-0 flex-1 flex-col p-3">
            {pageView === "gantt" ? (
              <PlanningBoard
                gantt={ganttSnapshot}
                commitments={commitments}
                capacityLoad={capacityLoad}
                lockedWindows={ganttSnapshot?.locked_windows ?? planningWindows}
                visibleStart={visibleStart}
                visibleEnd={visibleEnd}
                horizonStart={horizonStart}
                horizonEnd={horizonEnd}
                onVisibleTimeChange={(start, end) => {
                  // Keep pan/zoom inside the selected DU/AU horizon.
                  const clampedStart = Math.max(horizonStart, start);
                  const clampedEnd = Math.min(horizonEnd, end);
                  if (clampedEnd > clampedStart) {
                    setVisibleStart(clampedStart);
                    setVisibleEnd(clampedEnd);
                  } else {
                    const next = clampVisibleToHorizon(
                      horizonStart,
                      horizonEnd,
                      PLANNING_TIMELINE_MAX_ZOOM_MS,
                    );
                    setVisibleStart(next.start);
                    setVisibleEnd(next.end);
                  }
                }}
                selectedCommitmentId={selectedCommitmentId}
                onSelectCommitment={(id) => {
                  setSelectedCommitmentId(id);
                  setInspectorOpen(true);
                }}
                onReschedule={handleBoardReschedule}
                loading={loading && !ganttSnapshot}
                error={null}
                canDrag
                laneMode={boardScope.kind === "all" ? "crew" : "technician"}
                remountKey={`${dateFrom}_${dateTo}_${boardScope.kind === "all" ? "all" : boardScope.teamId}`}
                maxZoom={PLANNING_TIMELINE_MAX_ZOOM_MS}
              />
            ) : (
              <PlanningCalendarView
                key={`planning-calendar-${calendarSessionKey}`}
                commitments={calendarCommitments}
                windows={calendarWindows}
                loading={loading}
                initialDateIso={dateFrom}
                onVisibleRangeChange={handleCalendarVisibleRangeChange}
                onCommitmentClick={(commitment) => {
                  setSelectedCommitmentId(commitment.id);
                  setInspectorOpen(true);
                }}
                onWindowClick={(window) => {
                  setSelectedWindow(window);
                  setWindowDetailOpen(true);
                }}
              />
            )}
          </div>
        </div>
      </div>

      <CommitDialog
        open={commitOpen}
        onOpenChange={setCommitOpen}
        candidate={selectedCandidate}
        boardScope={boardScope}
        teams={teams}
        teamId={teamId}
        conflictSummary={
          selectedCandidate
            ? ((backlog?.conflict_summary ?? []).find(
                (row) => row.candidate_id === selectedCandidate.id,
              ) ?? null)
            : null
        }
        gantt={ganttSnapshot}
        commitments={commitments}
        lockedWindows={ganttSnapshot?.locked_windows ?? planningWindows}
        horizonStart={horizonStart}
        horizonEnd={horizonEnd}
        visibleStart={visibleStart}
        visibleEnd={visibleEnd}
        saving={saving}
        onSubmit={handleCreateCommitment}
      />
      <ProductionWindowDialog
        open={windowOpen}
        onOpenChange={setWindowOpen}
        teamId={teamId}
        windows={planningWindows}
        saving={saving}
        onSubmit={handleCreateWindow}
      />
      <BreakInDialog
        open={breakInOpen}
        onOpenChange={(next) => {
          setBreakInOpen(next);
          if (!next) setBreakInCommitmentId(null);
        }}
        commitments={commitments}
        breakIns={breakIns}
        teamId={teamId}
        initialCommitmentId={breakInCommitmentId}
        approvers={approvers}
        personnel={personnel}
        saving={saving}
        onSubmit={handleCreateBreakIn}
      />
      <FreezeConfirmDialog
        open={freezeOpen}
        onOpenChange={setFreezeOpen}
        periodStartLabel={new Date(periodStartIso).toLocaleString()}
        periodEndLabel={new Date(periodEndIso).toLocaleString()}
        saving={saving}
        onConfirm={handleFreeze}
      />
      <CommitmentInspectorDialog
        open={inspectorOpen}
        onOpenChange={setInspectorOpen}
        commitment={selectedCommitment}
        changeLog={changeLog}
        loadingChangeLog={loading}
        onLoadChangeLog={(id) => {
          void loadChangeLog(id);
        }}
        onRequestBreakIn={() => {
          setInspectorOpen(false);
          setBreakInCommitmentId(selectedCommitment?.id ?? null);
          setBreakInOpen(true);
        }}
        {...(pageView === "calendar" ? { onOpenOnBoard: handleOpenOnBoard } : {})}
      />
      <PlanningWindowDetailDialog
        open={windowDetailOpen}
        onOpenChange={(open) => {
          setWindowDetailOpen(open);
          if (!open) setSelectedWindow(null);
        }}
        window={selectedWindow}
      />
      <OverrideReasonDialog
        open={overrideOpen}
        onOpenChange={setOverrideOpen}
        onConfirm={(reason) => {
          const pending = pendingRescheduleRef.current;
          if (!pending) return;
          pendingRescheduleRef.current = null;
          void applyReschedule({
            ...pending.input,
            allow_double_booking_override: true,
            override_reason: reason,
          });
        }}
      />
    </div>
  );
}

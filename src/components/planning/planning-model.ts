/**
 * Planning board presentation helpers — adapters only; no IPC.
 */

import type { TFunction } from "i18next";

import { PERMIT_QUERY_CREATE_FOR_WO, PERMIT_QUERY_SELECT } from "@/components/permits/permit-rules";
import type { ResourceTimelineState } from "@/design-system/resource-timeline-tokens";
import {
  RESOURCE_TIMELINE_HORIZON_PRESETS,
  blockTypeToTimelineState,
} from "@/design-system/resource-timeline-tokens";
import type {
  ConflictReason,
  PlanningGanttSnapshot,
  PlanningWindow,
  ScheduleCandidate,
  ScheduleCommitment,
  TeamCapacityLoad,
} from "@shared/ipc-types";

export const PLANNING_HORIZON_PRESETS = RESOURCE_TIMELINE_HORIZON_PRESETS;
export const PLANNING_DEFAULT_HORIZON_DAYS = RESOURCE_TIMELINE_HORIZON_PRESETS[1]; // 14

/** Utilization ratio thresholds for capacity strip tone. */
export const PLANNING_UTILIZATION_PARTIAL = 0.8;
export const PLANNING_UTILIZATION_CRITICAL = 1;

export const PLANNING_UNASSIGNED_GROUP_ID = "unassigned";

export type SuggestedAssignee = {
  personnel_id: number;
  full_name: string;
  matching_skills: number;
};

export type BlockedInterval = {
  start_at: string;
  end_at: string;
  block_type: string;
  critical: boolean;
};

export type LaneCommitmentSnippet = {
  id: number;
  start: string;
  end: string;
  source_type: string;
  source_id: number;
};

export function toDateTimeLocal(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

export function fromDateTimeLocal(local: string): string {
  const d = new Date(local);
  return Number.isNaN(d.getTime()) ? local : d.toISOString();
}

/** Inclusive local calendar-day start (wall clock) → epoch ms. */
export function localDateStartMs(isoDate: string): number {
  const ms = Date.parse(`${isoDate}T00:00:00`);
  return Number.isNaN(ms) ? 0 : ms;
}

/** Inclusive local calendar-day end (wall clock) → epoch ms. */
export function localDateEndMs(isoDate: string): number {
  const ms = Date.parse(`${isoDate}T23:59:59.999`);
  return Number.isNaN(ms) ? 0 : ms;
}

/** Local date bounds as RFC3339 for planning IPC period filters. */
export function localDateStartIso(isoDate: string): string {
  const ms = localDateStartMs(isoDate);
  return ms ? new Date(ms).toISOString() : `${isoDate}T00:00:00.000Z`;
}

export function localDateEndIso(isoDate: string): string {
  const ms = localDateEndMs(isoDate);
  return ms ? new Date(ms).toISOString() : `${isoDate}T23:59:59.999Z`;
}

/**
 * Visible span for ResourceTimeline: full DU/AU when within maxZoom,
 * otherwise clamp to the leading window so the viewport never drifts
 * past the selected period.
 */
export function clampVisibleToHorizon(
  horizonStart: number,
  horizonEnd: number,
  maxZoomMs: number,
): { start: number; end: number } {
  if (!horizonStart || !horizonEnd || horizonEnd <= horizonStart) {
    return { start: horizonStart, end: horizonEnd };
  }
  const span = horizonEnd - horizonStart;
  if (span <= maxZoomMs) {
    return { start: horizonStart, end: horizonEnd };
  }
  return { start: horizonStart, end: horizonStart + maxZoomMs };
}

/** Planning boards may span a full year; Availability stays at 90d default. */
export const PLANNING_TIMELINE_MAX_ZOOM_MS = 366 * 24 * 60 * 60 * 1000;

export function addDaysIso(fromIsoDate: string, days: number): string {
  const parts = fromIsoDate.split("-").map(Number);
  const y = parts[0];
  const m = parts[1];
  const d = parts[2];
  if (y == null || m == null || d == null) return fromIsoDate;
  const dt = new Date(y, m - 1, d);
  if (Number.isNaN(dt.getTime())) return fromIsoDate;
  dt.setDate(dt.getDate() + days);
  const yy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

export function toLocalIsoDate(d: Date = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Per-user sticky board crew (legacy). Prefer boardScope storage. */
export const PLANNING_BOARD_CREW_STORAGE_PREFIX = "maintafox.planning.boardCrewId";

/** Per-user sticky board scope: All crews vs single crew. */
export const PLANNING_BOARD_SCOPE_STORAGE_PREFIX = "maintafox.planning.boardScope";

/** Per-user sticky page presentation: Gantt board vs calendar overview. */
export const PLANNING_PAGE_VIEW_STORAGE_PREFIX = "maintafox.planning.pageView";

export type PlanningBoardScope = { kind: "all" } | { kind: "crew"; teamId: number };

/** "week": the site's weekly schedule (approve, freeze, publish). */
export type PlanningPageView = "gantt" | "calendar" | "week";

export type PlanningCalendarMode = "month" | "week" | "day";

export type PlanningCalendarCommitmentTone = "frozen" | "conflict" | "normal";

export type PlanningCalendarWindowTone =
  | "freeze"
  | "shutdown"
  | "maintenance"
  | "production"
  | "other";

/** Chip left-border classes (same presentation approach as PmCalendarView). */
export const PLANNING_CALENDAR_COMMITMENT_BORDER: Record<PlanningCalendarCommitmentTone, string> = {
  frozen: "border-l-slate-500",
  conflict: "border-l-red-500",
  normal: "border-l-blue-500",
};

export const PLANNING_CALENDAR_WINDOW_BORDER: Record<PlanningCalendarWindowTone, string> = {
  freeze: "border-l-violet-500",
  shutdown: "border-l-orange-500",
  maintenance: "border-l-amber-500",
  production: "border-l-emerald-500",
  other: "border-l-gray-400",
};

export function planningBoardCrewStorageKey(userId: number | null | undefined): string {
  if (userId != null && Number.isFinite(userId)) {
    return `${PLANNING_BOARD_CREW_STORAGE_PREFIX}.${userId}`;
  }
  return PLANNING_BOARD_CREW_STORAGE_PREFIX;
}

export function planningBoardScopeStorageKey(userId: number | null | undefined): string {
  if (userId != null && Number.isFinite(userId)) {
    return `${PLANNING_BOARD_SCOPE_STORAGE_PREFIX}.${userId}`;
  }
  return PLANNING_BOARD_SCOPE_STORAGE_PREFIX;
}

export function readStoredBoardCrewId(userId: number | null | undefined): number | null {
  try {
    const raw = localStorage.getItem(planningBoardCrewStorageKey(userId));
    if (!raw) return null;
    const n = Number(raw);
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

export function writeStoredBoardCrewId(
  userId: number | null | undefined,
  teamId: number | null,
): void {
  try {
    const key = planningBoardCrewStorageKey(userId);
    if (teamId == null) {
      localStorage.removeItem(key);
      return;
    }
    localStorage.setItem(key, String(teamId));
  } catch {
    /* private mode / quota — ignore */
  }
}

export function serializeBoardScope(scope: PlanningBoardScope): string {
  return scope.kind === "all" ? "all" : `crew:${scope.teamId}`;
}

export function parseBoardScope(raw: string | null | undefined): PlanningBoardScope | null {
  if (!raw) return null;
  if (raw === "all") return { kind: "all" };
  if (raw.startsWith("crew:")) {
    const n = Number(raw.slice(5));
    if (Number.isFinite(n) && n > 0) return { kind: "crew", teamId: n };
  }
  return null;
}

/**
 * Read sticky board scope. Migrates legacy `boardCrewId` once into `boardScope`.
 * Defaults to All when nothing stored.
 */
export function readStoredBoardScope(userId: number | null | undefined): PlanningBoardScope {
  try {
    const scopeKey = planningBoardScopeStorageKey(userId);
    const raw = localStorage.getItem(scopeKey);
    const parsed = parseBoardScope(raw);
    if (parsed) return parsed;

    const legacy = readStoredBoardCrewId(userId);
    if (legacy != null) {
      const migrated: PlanningBoardScope = { kind: "crew", teamId: legacy };
      writeStoredBoardScope(userId, migrated);
      return migrated;
    }
  } catch {
    /* ignore */
  }
  return { kind: "all" };
}

export function writeStoredBoardScope(
  userId: number | null | undefined,
  scope: PlanningBoardScope,
): void {
  try {
    localStorage.setItem(planningBoardScopeStorageKey(userId), serializeBoardScope(scope));
    // Keep legacy key in sync for older readers.
    if (scope.kind === "crew") {
      writeStoredBoardCrewId(userId, scope.teamId);
    } else {
      writeStoredBoardCrewId(userId, null);
    }
  } catch {
    /* private mode / quota — ignore */
  }
}

export function planningPageViewStorageKey(userId: number | null | undefined): string {
  if (userId != null && Number.isFinite(userId)) {
    return `${PLANNING_PAGE_VIEW_STORAGE_PREFIX}.${userId}`;
  }
  return PLANNING_PAGE_VIEW_STORAGE_PREFIX;
}

export function readStoredPageView(userId: number | null | undefined): PlanningPageView {
  try {
    const raw = localStorage.getItem(planningPageViewStorageKey(userId));
    if (raw === "calendar" || raw === "gantt" || raw === "week") return raw;
  } catch {
    /* ignore */
  }
  return "gantt";
}

export function writeStoredPageView(
  userId: number | null | undefined,
  view: PlanningPageView,
): void {
  try {
    localStorage.setItem(planningPageViewStorageKey(userId), view);
  } catch {
    /* private mode / quota — ignore */
  }
}

/** Local calendar day key `YYYY-MM-DD` from a Date (wall clock). */
export function localDayKeyFromDate(d: Date): string {
  return toLocalIsoDate(d);
}

/** Local calendar day key from an RFC3339 / ISO datetime; null if invalid. */
export function localDayKeyFromIso(iso: string): string | null {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return null;
  return localDayKeyFromDate(new Date(ms));
}

/**
 * Inclusive local-day keys covering [startIso, endIso].
 * Empty when either bound is invalid or end is before start.
 */
export function eachLocalDayKey(startIso: string, endIso: string): string[] {
  const startKey = localDayKeyFromIso(startIso);
  const endKey = localDayKeyFromIso(endIso);
  if (!startKey || !endKey) return [];
  if (startKey > endKey) return [];

  const keys: string[] = [];
  let cursor = startKey;
  // Safety cap: ~3 years of days for a single span.
  for (let i = 0; i < 1100; i++) {
    keys.push(cursor);
    if (cursor === endKey) break;
    cursor = addDaysIso(cursor, 1);
  }
  return keys;
}

export function calendarCommitmentTone(
  commitment: ScheduleCommitment,
): PlanningCalendarCommitmentTone {
  if (commitment.frozen_at) return "frozen";
  if (commitment.has_blocking_conflict) return "conflict";
  return "normal";
}

export function calendarWindowTone(window: PlanningWindow): PlanningCalendarWindowTone {
  const type = window.window_type?.trim().toLowerCase() ?? "";
  if (type === "freeze") return "freeze";
  if (type === "shutdown") return "shutdown";
  if (type === "maintenance") return "maintenance";
  if (type === "production") return "production";
  return "other";
}

export function commitmentCalendarLabel(commitment: ScheduleCommitment): string {
  if (commitment.source_code?.trim()) return commitment.source_code.trim();
  if (commitment.source_title?.trim()) return commitment.source_title.trim();
  return commitment.source_type;
}

export function filterCommitmentsForBoardScope(
  commitments: ScheduleCommitment[],
  scope: PlanningBoardScope,
): ScheduleCommitment[] {
  if (scope.kind === "all") return commitments;
  return commitments.filter((c) => c.assigned_team_id === scope.teamId);
}

/** Monday-start week containing `d` (local). */
export function startOfPlanningWeek(d: Date): Date {
  const day = d.getDay();
  const diff = d.getDate() - day + (day === 0 ? -6 : 1);
  return new Date(d.getFullYear(), d.getMonth(), diff);
}

/** Inclusive From–To ISO dates for the calendar cursor + mode (drives board reload). */
export function calendarVisibleRangeIso(
  cursor: Date,
  mode: PlanningCalendarMode,
): { dateFrom: string; dateTo: string } {
  if (mode === "day") {
    const key = localDayKeyFromDate(cursor);
    return { dateFrom: key, dateTo: key };
  }
  if (mode === "week") {
    const start = startOfPlanningWeek(cursor);
    const from = localDayKeyFromDate(start);
    return { dateFrom: from, dateTo: addDaysIso(from, 6) };
  }
  const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
  const last = new Date(cursor.getFullYear(), cursor.getMonth() + 1, 0);
  return {
    dateFrom: localDayKeyFromDate(first),
    dateTo: localDayKeyFromDate(last),
  };
}

export function parseLocalIsoDateToDate(isoDate: string): Date {
  const parts = isoDate.split("-").map(Number);
  const y = parts[0];
  const m = parts[1];
  const d = parts[2];
  if (y == null || m == null || d == null) return new Date();
  const dt = new Date(y, m - 1, d);
  return Number.isNaN(dt.getTime()) ? new Date() : dt;
}

export const PLANNING_TEAM_GROUP_PREFIX = "team-";

export function teamGroupId(teamId: number): string {
  return `${PLANNING_TEAM_GROUP_PREFIX}${teamId}`;
}

export function parseTeamGroupId(groupId: string | number): number | null {
  const raw = String(groupId);
  if (!raw.startsWith(PLANNING_TEAM_GROUP_PREFIX)) return null;
  const n = Number(raw.slice(PLANNING_TEAM_GROUP_PREFIX.length));
  return Number.isFinite(n) ? n : null;
}

/** @deprecated Use teamGroupId — kept as alias for transitional imports. */
export const PLANNING_CREW_GROUP_PREFIX = PLANNING_TEAM_GROUP_PREFIX;
/** @deprecated Use teamGroupId */
export const crewGroupId = teamGroupId;
/** @deprecated Use parseTeamGroupId */
export const parseCrewGroupId = parseTeamGroupId;

/** Local calendar day `YYYY-MM-DD` from an epoch ms instant. */
export function msToLocalIsoDate(ms: number): string {
  if (!ms) return "";
  return toLocalIsoDate(new Date(ms));
}

/** Hours between two datetime-local / ISO strings; null if invalid. */
export function hoursBetweenLocals(startLocal: string, endLocal: string): number | null {
  if (!startLocal.trim() || !endLocal.trim()) return null;
  const startMs = parseMs(fromDateTimeLocal(startLocal));
  const endMs = parseMs(fromDateTimeLocal(endLocal));
  if (!startMs || !endMs || endMs <= startMs) return null;
  return Math.round(((endMs - startMs) / 3_600_000) * 100) / 100;
}

/** Mirror Rust estimate_labor_cost rounding: hours × rate → 2 decimals. */
export function previewLaborCost(hours: number, laborRate: number): number {
  return Math.round(hours * laborRate * 100) / 100;
}

/** Keep digits and at most one decimal separator for budget threshold input. */
export function sanitizeBudgetThresholdInput(raw: string): string {
  const cleaned = raw.replace(/[^\d.]/g, "");
  const firstDot = cleaned.indexOf(".");
  if (firstDot < 0) return cleaned;
  return cleaned.slice(0, firstDot + 1) + cleaned.slice(firstDot + 1).replace(/\./g, "");
}

export function parseMs(iso: string): number {
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? 0 : ms;
}

export function commitmentItemId(id: number): string {
  return `cmt-${id}`;
}

export function parseCommitmentItemId(itemId: string | number): number | null {
  const raw = String(itemId);
  if (!raw.startsWith("cmt-")) return null;
  const n = Number(raw.slice(4));
  return Number.isFinite(n) ? n : null;
}

export function blockItemId(personnelId: number, index: number): string {
  return `blk-${personnelId}-${index}`;
}

export function candidateDisplayCode(candidate: ScheduleCandidate): string {
  if (candidate.source_code?.trim()) return candidate.source_code.trim();
  return candidate.source_type;
}

export function candidateDisplayTitle(candidate: ScheduleCandidate): string {
  if (candidate.source_title?.trim()) return candidate.source_title.trim();
  return "";
}

export function parseSuggestedAssignees(json: string | null | undefined): SuggestedAssignee[] {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((row) => {
        if (!row || typeof row !== "object") return null;
        const rec = row as Record<string, unknown>;
        const personnel_id = Number(rec["personnel_id"]);
        if (!Number.isFinite(personnel_id)) return null;
        return {
          personnel_id,
          full_name: typeof rec["full_name"] === "string" ? rec["full_name"] : "",
          matching_skills: Number(rec["matching_skills"]) || 0,
        };
      })
      .filter((row): row is SuggestedAssignee => row != null);
  } catch {
    return [];
  }
}

export function parseBlockedIntervals(json: string | null | undefined): BlockedInterval[] {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((row) => {
        if (!row || typeof row !== "object") return null;
        const rec = row as Record<string, unknown>;
        const start_at = typeof rec["start_at"] === "string" ? rec["start_at"] : "";
        const end_at = typeof rec["end_at"] === "string" ? rec["end_at"] : "";
        if (!start_at || !end_at) return null;
        return {
          start_at,
          end_at,
          block_type: typeof rec["block_type"] === "string" ? rec["block_type"] : "leave",
          critical: Boolean(rec["critical"]),
        };
      })
      .filter((row): row is BlockedInterval => row != null);
  } catch {
    return [];
  }
}

export function utilizationState(ratio: number): ResourceTimelineState {
  if (ratio >= PLANNING_UTILIZATION_CRITICAL) return "critical";
  if (ratio >= PLANNING_UTILIZATION_PARTIAL) return "partial";
  return "free";
}

export function capacityByDate(rows: TeamCapacityLoad[]): Map<string, TeamCapacityLoad> {
  const map = new Map<string, TeamCapacityLoad>();
  for (const row of rows) {
    map.set(row.work_date.slice(0, 10), row);
  }
  return map;
}

/** Stable id for the in-dialog proposed commitment preview bar. */
export const PLANNING_PROPOSED_ITEM_ID = "proposed-commit";

export type PlanningTimelineLane = {
  id: string;
  /** null = unassigned / team bucket lane */
  personnelId: number | null;
  fullName: string | null;
};

export type PlanningTimelineItemData = {
  id: string;
  groupId: string;
  title: string;
  startMs: number;
  endMs: number;
  state: ResourceTimelineState;
  canMove: boolean;
  /** Why the bar is critical: its open conflicts, one per line. */
  conflictReasons?: ConflictReason[];
};

export type PlanningWindowHighlightData = {
  start: number;
  end: number;
  state: "restriction";
};

/** Lanes: unassigned bucket first, then gantt assignee rows. */
export function buildPlanningTimelineLanes(
  gantt: PlanningGanttSnapshot | null | undefined,
): PlanningTimelineLane[] {
  const lanes: PlanningTimelineLane[] = [
    {
      id: PLANNING_UNASSIGNED_GROUP_ID,
      personnelId: null,
      fullName: null,
    },
  ];
  for (const lane of gantt?.assignee_lanes ?? []) {
    lanes.push({
      id: String(lane.personnel_id),
      personnelId: lane.personnel_id,
      fullName: lane.full_name,
    });
  }
  return lanes;
}

/** Commitment bars + availability blocks for ResourceTimeline. */
export function buildPlanningTimelineItems(input: {
  commitments: ScheduleCommitment[];
  gantt: PlanningGanttSnapshot | null | undefined;
  canDrag?: boolean;
}): PlanningTimelineItemData[] {
  const { commitments, gantt, canDrag = false } = input;
  const next: PlanningTimelineItemData[] = [];

  for (const commitment of commitments) {
    const start = parseMs(commitment.committed_start);
    const end = parseMs(commitment.committed_end);
    if (!start || !end || end <= start) continue;
    const frozen = Boolean(commitment.frozen_at);
    const title =
      commitment.source_code?.trim() || commitment.source_title?.trim() || commitment.source_type;
    next.push({
      id: commitmentItemId(commitment.id),
      groupId:
        commitment.assigned_personnel_id != null
          ? String(commitment.assigned_personnel_id)
          : PLANNING_UNASSIGNED_GROUP_ID,
      title,
      startMs: start,
      endMs: end,
      state: commitmentTimelineState(commitment),
      canMove: canDrag && !frozen,
      conflictReasons: parseConflictReasons(commitment.conflict_reasons_json),
    });
  }

  for (const lane of gantt?.assignee_lanes ?? []) {
    const intervals = parseBlockedIntervals(lane.blocked_intervals_json);
    intervals.forEach((interval, index) => {
      const start = parseMs(interval.start_at);
      const end = parseMs(interval.end_at);
      if (!start || !end || end <= start) return;
      next.push({
        id: blockItemId(lane.personnel_id, index),
        groupId: String(lane.personnel_id),
        title: interval.block_type,
        startMs: start,
        endMs: end,
        state: blockTypeToTimelineState(interval.block_type, interval.critical),
        canMove: false,
      });
    });
  }

  return next;
}

export function buildPlanningWindowHighlights(
  lockedWindows: PlanningWindow[],
): PlanningWindowHighlightData[] {
  return lockedWindows
    .filter((w) => w.is_locked === 1)
    .map((w) => ({
      start: parseMs(w.start_datetime),
      end: parseMs(w.end_datetime),
      state: "restriction" as const,
    }))
    .filter((h) => h.start > 0 && h.end > h.start);
}

/**
 * Prefill commit slot from the candidate's window and estimated duration — never an assumed
 * length: without an estimate only the known ends are filled. The dialog then keeps the end
 * inside the team's remaining capacity of the day (its Classes horaires + overtime ceiling).
 */
export function deriveCommitSlotLocals(candidate: ScheduleCandidate | null): {
  startLocal: string;
  endLocal: string;
} {
  if (!candidate) return { startLocal: "", endLocal: "" };

  const windowStart = candidate.window_start?.trim() ? candidate.window_start : null;
  const windowEnd = candidate.window_end?.trim() ? candidate.window_end : null;
  const durationH = candidate.estimated_duration_hours;
  const hasDuration = durationH != null && Number.isFinite(durationH) && durationH > 0;

  const endFromStart = (startIso: string, hours: number, hardEndIso?: string | null) => {
    const startMs = parseMs(startIso);
    if (!startMs) return "";
    let endMs = startMs + hours * 3_600_000;
    if (hardEndIso) {
      const hardEnd = parseMs(hardEndIso);
      if (hardEnd && hardEnd > startMs) endMs = Math.min(endMs, hardEnd);
    }
    return toDateTimeLocal(new Date(endMs).toISOString());
  };

  if (windowStart && windowEnd) {
    return {
      startLocal: toDateTimeLocal(windowStart),
      endLocal: hasDuration
        ? endFromStart(windowStart, durationH, windowEnd)
        : toDateTimeLocal(windowEnd),
    };
  }
  if (windowStart) {
    return {
      startLocal: toDateTimeLocal(windowStart),
      endLocal: hasDuration ? endFromStart(windowStart, durationH) : "",
    };
  }
  if (windowEnd) {
    const endMs = parseMs(windowEnd);
    return {
      startLocal:
        hasDuration && endMs
          ? toDateTimeLocal(new Date(endMs - durationH * 3_600_000).toISOString())
          : "",
      endLocal: toDateTimeLocal(windowEnd),
    };
  }
  return { startLocal: "", endLocal: "" };
}

/** Local calendar day (`YYYY-MM-DD`) of an instant — the days the timetables and the load use. */
export function localDayOf(iso: string): string | null {
  const ms = parseMs(iso);
  if (!ms) return null;
  const d = new Date(ms);
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

export type TeamDayCapacity = {
  availableHours: number;
  overtimeHours: number;
  committedHours: number;
  remainingHours: number;
  /** `roster` | `rest_day` | `holiday` | `no_schedule` | `no_members`; null when not loaded. */
  basis: string | null;
};

/**
 * A team's capacity on a local day (the members' Classes horaires on the site's working days −
 * absences, × utilisation, plus the planner's overtime ceiling) and the labour hours already
 * committed there. No row ⇒ 0 h, never a default.
 */
export function resolveTeamDayCapacity(
  capacity: TeamCapacityLoad[] | null | undefined,
  teamId: number,
  workDate: string,
): TeamDayCapacity {
  const row = (capacity ?? []).find((c) => c.team_id === teamId && c.work_date === workDate);
  const availableHours = row?.available_hours ?? 0;
  const overtimeHours = row?.overtime_hours ?? 0;
  const committedHours = row?.committed_hours ?? 0;
  const remainingHours = Math.max(0, availableHours + overtimeHours - committedHours);
  return {
    availableHours,
    overtimeHours,
    committedHours,
    remainingHours,
    basis: row?.basis ? row.basis : null,
  };
}

/** The local days a slot covers, with the share (0–1] of each (mirrors `planning::load::slot_days`). */
export function slotDays(startIso: string, endIso: string): Array<{ day: string; share: number }> {
  const startMs = parseMs(startIso);
  const endMs = parseMs(endIso);
  if (!startMs || !endMs || endMs <= startMs) return [];
  const out: Array<{ day: string; share: number }> = [];
  const cursor = new Date(startMs);
  cursor.setHours(0, 0, 0, 0);
  while (cursor.getTime() < endMs) {
    const dayStart = cursor.getTime();
    const next = new Date(cursor);
    next.setDate(next.getDate() + 1);
    const dayEnd = next.getTime();
    const covered = Math.min(endMs, dayEnd) - Math.max(startMs, dayStart);
    const key = localDayOf(new Date(dayStart).toISOString());
    if (covered > 0 && key) {
      out.push({ day: key, share: Math.min(1, covered / (dayEnd - dayStart)) });
    }
    cursor.setTime(dayEnd);
  }
  return out;
}

/** A capacity window's local wall-clock time (`YYYY-MM-DDTHH:MM:SS`, no offset ⇒ local) in ms. */
function localWallMs(raw: string): number | null {
  const ms = new Date(raw).getTime();
  return Number.isFinite(ms) ? ms : null;
}

function overlapMs(aStart: number, aEnd: number, bStart: number, bEnd: number): number {
  return Math.max(0, Math.min(aEnd, bEnd) - Math.max(aStart, bStart));
}

/**
 * Spreads labour hours over working time (mirrors `planning::load::spread`): each day weighs its
 * members' shift windows by the part inside the slot (a night shift belongs to the day it starts);
 * hours of members without a known window count in full on the days the slot touches. Only a slot
 * touching no working time at all keeps its hours on the calendar days it covers (the overload shows).
 */
export function spreadLabour(
  labourHours: number,
  startIso: string,
  endIso: string,
  teamRows: TeamCapacityLoad[],
): Map<string, number> {
  const calendar = slotDays(startIso, endIso);
  const out = new Map<string, number>();
  const startMs = parseMs(startIso);
  const endMs = parseMs(endIso);
  if (calendar.length === 0 || !startMs || !endMs) return out;
  const touched = new Set(calendar.map((d) => d.day));
  let weights: Array<{ day: string; w: number }> = [];
  for (const row of teamRows) {
    let w = 0;
    for (const win of row.work_windows) {
      const s = localWallMs(win.start);
      const e = localWallMs(win.end);
      if (s == null || e == null || e <= s) continue;
      w += (win.hours * overlapMs(startMs, endMs, s, e)) / (e - s);
    }
    if (row.unwindowed_hours > 0 && touched.has(row.work_date)) w += row.unwindowed_hours;
    if (w > 0) weights.push({ day: row.work_date, w });
  }
  if (weights.length === 0) weights = calendar.map((d) => ({ day: d.day, w: d.share }));
  const total = weights.reduce((sum, d) => sum + d.w, 0);
  if (total <= 0) return out;
  for (const { day, w } of weights) out.set(day, (out.get(day) ?? 0) + (labourHours * w) / total);
  return out;
}

/**
 * Shift time one person can give the slot: each day, the longest part of a member's shift window
 * inside the slot, summed (hours). Null when a day it touches has hours without a known window.
 */
export function slotShiftHours(
  startIso: string,
  endIso: string,
  teamRows: TeamCapacityLoad[],
): number | null {
  const startMs = parseMs(startIso);
  const endMs = parseMs(endIso);
  if (!startMs || !endMs || endMs <= startMs) return null;
  const touched = new Set(slotDays(startIso, endIso).map((d) => d.day));
  let total = 0;
  for (const row of teamRows) {
    if (row.unwindowed_hours > 0 && touched.has(row.work_date)) return null;
    let longest = 0;
    for (const win of row.work_windows) {
      const s = localWallMs(win.start);
      const e = localWallMs(win.end);
      if (s == null || e == null) continue;
      longest = Math.max(longest, overlapMs(startMs, endMs, s, e));
    }
    total += longest / 3_600_000;
  }
  return total;
}

export type SlotCapacityDay = TeamDayCapacity & { day: string; shareHours: number };

/** The previous local day (`YYYY-MM-DD`): a night shift that started then may run into a slot. */
function previousDay(day: string): string {
  const d = new Date(`${day}T12:00:00`);
  d.setDate(d.getDate() - 1);
  return localDayOf(d.toISOString()) ?? day;
}

/** The team's capacity rows a slot can use: the day before it starts through its last day. */
export function slotCapacityRows(
  capacity: TeamCapacityLoad[] | null | undefined,
  teamId: number,
  startIso: string,
  endIso: string,
): TeamCapacityLoad[] {
  const days = slotDays(startIso, endIso);
  const usable = new Set(days.map((d) => d.day));
  const first = days[0]?.day;
  if (first) usable.add(previousDay(first));
  return (capacity ?? []).filter((c) => c.team_id === teamId && usable.has(c.work_date));
}

/**
 * Capacity over the days a slot uses, as the backend gate checks it: the work's labour hours
 * (duration × crew size) spread over the working time it covers, against each day's remaining hours.
 */
export function checkSlotCapacity(args: {
  capacity: TeamCapacityLoad[] | null | undefined;
  teamId: number;
  startIso: string;
  endIso: string;
  labourHours: number;
}): { days: SlotCapacityDay[]; overloaded: SlotCapacityDay | null; noSchedule: boolean } {
  const { capacity, teamId, startIso, endIso, labourHours } = args;
  const days = slotDays(startIso, endIso).map((d) => d.day);
  const shares = spreadLabour(
    labourHours,
    startIso,
    endIso,
    slotCapacityRows(capacity, teamId, startIso, endIso),
  );
  const shown = [...new Set([...days, ...shares.keys()])].sort();
  const rows: SlotCapacityDay[] = shown.map((day) => ({
    ...resolveTeamDayCapacity(capacity, teamId, day),
    day,
    shareHours: shares.get(day) ?? 0,
  }));
  const overloaded = rows.find((r) => r.shareHours > r.remainingHours + 0.01) ?? null;
  const slotRows = rows.filter((r) => days.includes(r.day));
  const noSchedule =
    slotRows.length > 0 &&
    slotRows.every((r) => r.basis === "no_schedule" || r.basis === "no_members");
  return { days: rows, overloaded, noSchedule };
}

/** Assignee refusals the planner may override with a reason (the commit gate's codes). */
export const ASSIGNEE_OVERRIDABLE_CODES = [
  "PLANNING_ASSIGNEE_NOT_IN_CREW",
  "PLANNING_ASSIGNEE_NOT_WORKING",
  "PLANNING_ASSIGNEE_OVERLOADED",
] as const;

/** A refusal about the technician (crew, shift, hours left, double booking): overridable with a reason. */
export function isAssigneeOverridableError(codes: string[], message: string): boolean {
  return (
    codes.some((c) => (ASSIGNEE_OVERRIDABLE_CODES as readonly string[]).includes(c)) ||
    isDoubleBookingErrorMessage(message)
  );
}

export function isDoubleBookingErrorMessage(message: string): boolean {
  const lower = message.toLowerCase();
  return lower.includes("double-booking") || lower.includes("double booking");
}

export function isCapacityGateErrorMessage(message: string): boolean {
  const lower = message.toLowerCase();
  return lower.includes("capacity gate") || lower.includes("exceeds capacity");
}

export function parseLaborCostRange(
  json: string | null | undefined,
): { min: number; max: number } | null {
  if (!json) return null;
  try {
    const parsed = JSON.parse(json) as Record<string, unknown>;
    const min = Number(parsed["min"] ?? parsed["low"] ?? parsed["from"]);
    const max = Number(parsed["max"] ?? parsed["high"] ?? parsed["to"]);
    if (!Number.isFinite(min) || !Number.isFinite(max)) return null;
    return { min, max };
  } catch {
    return null;
  }
}

/**
 * Screen that shows the record behind a planning candidate (OT detail, DI detail), or null
 * when the source has no deep-linkable screen yet (e.g. PM occurrences).
 */
export function candidateSourcePath(
  candidate: Pick<ScheduleCandidate, "source_type" | "source_id" | "source_di_id">,
): string | null {
  if (candidate.source_type === "work_order" && candidate.source_id > 0) {
    return `/work-orders?openWo=${candidate.source_id}`;
  }
  if (candidate.source_di_id != null && candidate.source_di_id > 0) {
    return `/requests?openDi=${candidate.source_di_id}`;
  }
  return null;
}

/** The open conflicts of a commitment (`conflict_reasons_json`); unreadable JSON gives none. */
export function parseConflictReasons(json: string | null | undefined): ConflictReason[] {
  if (!json) return [];
  try {
    const parsed: unknown = JSON.parse(json);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (r): r is ConflictReason =>
        typeof r === "object" &&
        r !== null &&
        typeof (r as ConflictReason).conflict_type === "string" &&
        typeof (r as ConflictReason).reason_code === "string",
    );
  } catch {
    return [];
  }
}

function reasonDetails(reason: ConflictReason): Record<string, unknown> {
  if (!reason.details_json) return {};
  try {
    const parsed: unknown = JSON.parse(reason.details_json);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

function stringList(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

/**
 * A conflict explained in the planner's words: the reason (`reasons.<REASON_CODE>`) with its
 * details, else the conflict type's label.
 */
export function conflictReasonText(reason: ConflictReason, t: TFunction<"planning">): string {
  const details = reasonDetails(reason);
  // Lists read as "a, b"; the parts of an estimate are translated.
  const params: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(details)) {
    params[key] = Array.isArray(value) ? stringList(value).join(", ") : value;
  }
  const skills = stringList(details["required_skills"]);
  const missing = stringList(details["missing"]).map((m) =>
    t(`reasonParts.${m}`, { defaultValue: m }),
  );
  const fallback = t(`blockers.${reason.conflict_type}`, { defaultValue: reason.conflict_type });
  return t(`reasons.${reason.reason_code}`, {
    defaultValue: fallback,
    ...params,
    missing: missing.join(", "),
    skills: skills.join(", "),
    required: skills.length,
  });
}

/** Hover text of a bar: its title, then why it is critical. */
export function timelineItemTooltip(
  item: Pick<PlanningTimelineItemData, "title" | "conflictReasons">,
  t: TFunction<"planning">,
): string {
  const reasons = (item.conflictReasons ?? []).map((r) => `• ${conflictReasonText(r, t)}`);
  return [item.title, ...reasons].join("\n");
}

/** Blocking refuses the commitment; any other severity than `warning` blocks (fails closed). */
export function isBlockingSeverity(severity: string): boolean {
  return severity !== "warning";
}

/** Where a reason leads: the permit, a stock article, or the planning windows. */
export type ConflictReasonLink =
  | { kind: "permit"; path: string }
  | { kind: "requestPermit"; path: string }
  | { kind: "article"; path: string; code: string }
  | { kind: "windows" };

export function conflictReasonLinks(reason: ConflictReason): ConflictReasonLink[] {
  const details = reasonDetails(reason);
  const permitId = details["permit_id"];
  const workOrderId = details["work_order_id"];
  if (typeof permitId === "number") {
    return [{ kind: "permit", path: `/permits?${PERMIT_QUERY_SELECT}=${permitId}` }];
  }
  if (reason.reason_code === "PERMIT_TO_REQUEST" && typeof workOrderId === "number") {
    return [
      { kind: "requestPermit", path: `/permits?${PERMIT_QUERY_CREATE_FOR_WO}=${workOrderId}` },
    ];
  }
  if (reason.reason_code === "NO_FEASIBLE_WINDOW") return [{ kind: "windows" }];
  const lines = Array.isArray(details["lines"]) ? (details["lines"] as unknown[]) : [];
  return lines.flatMap((line): ConflictReasonLink[] => {
    if (typeof line !== "object" || line === null) return [];
    const { article_id: articleId, article } = line as Record<string, unknown>;
    return typeof articleId === "number"
      ? [
          {
            kind: "article",
            path: `/inventory?articleId=${articleId}`,
            code: typeof article === "string" ? article : String(articleId),
          },
        ]
      : [];
  });
}

/**
 * Colour of a commitment's bar: frozen, critical (a blocking conflict now, or forced over one),
 * amber (warnings only — enforced later, e.g. the permit at OT start), or plain.
 */
export function commitmentTimelineState(
  commitment: Pick<
    ScheduleCommitment,
    "frozen_at" | "has_blocking_conflict" | "conflict_reasons_json"
  >,
): ResourceTimelineState {
  if (commitment.frozen_at) return "blocked";
  if (commitment.has_blocking_conflict) return "critical";
  return parseConflictReasons(commitment.conflict_reasons_json).length > 0
    ? "partial"
    : "commitment";
}

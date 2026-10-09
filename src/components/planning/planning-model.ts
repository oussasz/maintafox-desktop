/**
 * Planning board presentation helpers — adapters only; no IPC.
 */

import type { ResourceTimelineState } from "@/design-system/resource-timeline-tokens";
import {
  blockTypeToTimelineState,
  RESOURCE_TIMELINE_HORIZON_PRESETS,
} from "@/design-system/resource-timeline-tokens";
import type {
  PlanningGanttSnapshot,
  PlanningWindow,
  ScheduleCandidate,
  ScheduleCommitment,
  TeamCapacityLoad,
} from "@shared/ipc-types";

export const PLANNING_HORIZON_PRESETS = RESOURCE_TIMELINE_HORIZON_PRESETS;
export const PLANNING_DEFAULT_HORIZON_DAYS = RESOURCE_TIMELINE_HORIZON_PRESETS[1]; // 14

/** Default daily available hours when no capacity_rules row exists (matches Rust gate). */
export const PLANNING_DEFAULT_DAILY_CAPACITY_HOURS = 8;

/** Utilization ratio thresholds for capacity strip tone. */
export const PLANNING_UTILIZATION_PARTIAL = 0.8;
export const PLANNING_UTILIZATION_CRITICAL = 1;

export const PLANNING_UNASSIGNED_GROUP_ID = "unassigned";

export const PLANNING_BREAK_IN_REASONS = [
  "emergency",
  "safety",
  "production_loss",
  "regulatory",
  "other",
] as const;

export type PlanningBreakInReason = (typeof PLANNING_BREAK_IN_REASONS)[number];

export const PLANNING_WINDOW_TYPES = ["production", "maintenance", "shutdown", "freeze"] as const;

export type PlanningWindowType = (typeof PLANNING_WINDOW_TYPES)[number];

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

export type PlanningPageView = "gantt" | "calendar";

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
    if (raw === "calendar" || raw === "gantt") return raw;
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
      state: frozen ? "blocked" : commitment.has_blocking_conflict ? "critical" : "commitment",
      canMove: canDrag && !frozen,
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
 * Prefill commit slot from candidate window / duration.
 * Capacity gate is daily (≈8h + overtime) — never prefill a multi-day span that
 * would always fail the gate. Prefer estimated duration when the window is wider.
 */
export function deriveCommitSlotLocals(candidate: ScheduleCandidate | null): {
  startLocal: string;
  endLocal: string;
} {
  if (!candidate) return { startLocal: "", endLocal: "" };

  const windowStart = candidate.window_start?.trim() ? candidate.window_start : null;
  const windowEnd = candidate.window_end?.trim() ? candidate.window_end : null;
  const hasStart = windowStart != null;
  const hasEnd = windowEnd != null;
  const durationH = candidate.estimated_duration_hours;
  const maxSlotH = PLANNING_DEFAULT_DAILY_CAPACITY_HOURS;

  const clampEndFromStart = (startIso: string, preferredHours: number, hardEndIso?: string) => {
    const startMs = parseMs(startIso);
    if (!startMs) return "";
    let endMs = startMs + preferredHours * 3_600_000;
    if (hardEndIso) {
      const hardEnd = parseMs(hardEndIso);
      if (hardEnd && hardEnd > startMs) endMs = Math.min(endMs, hardEnd);
    }
    return toDateTimeLocal(new Date(endMs).toISOString());
  };

  if (hasStart && hasEnd) {
    const startMs = parseMs(windowStart);
    const endMs = parseMs(windowEnd);
    const spanH = startMs && endMs && endMs > startMs ? (endMs - startMs) / 3_600_000 : 0;
    const preferred =
      durationH != null && Number.isFinite(durationH) && durationH > 0
        ? Math.min(durationH, maxSlotH)
        : Math.min(spanH > 0 ? spanH : maxSlotH, maxSlotH);

    if (spanH > preferred + 0.01) {
      return {
        startLocal: toDateTimeLocal(windowStart),
        endLocal: clampEndFromStart(windowStart, preferred, windowEnd),
      };
    }

    return {
      startLocal: toDateTimeLocal(windowStart),
      endLocal: toDateTimeLocal(windowEnd),
    };
  }

  if (hasStart && !hasEnd && durationH != null && Number.isFinite(durationH) && durationH > 0) {
    const preferred = Math.min(durationH, maxSlotH);
    return {
      startLocal: toDateTimeLocal(windowStart),
      endLocal: clampEndFromStart(windowStart, preferred),
    };
  }

  if (hasEnd && !hasStart && durationH != null && Number.isFinite(durationH) && durationH > 0) {
    const endMs = parseMs(windowEnd);
    if (endMs) {
      const preferred = Math.min(durationH, maxSlotH);
      return {
        startLocal: toDateTimeLocal(new Date(endMs - preferred * 3_600_000).toISOString()),
        endLocal: toDateTimeLocal(windowEnd),
      };
    }
  }

  if (hasStart) {
    return {
      startLocal: toDateTimeLocal(windowStart),
      endLocal: clampEndFromStart(windowStart, maxSlotH),
    };
  }
  if (hasEnd) {
    return { startLocal: "", endLocal: toDateTimeLocal(windowEnd) };
  }

  return { startLocal: "", endLocal: "" };
}

/** UTC work-date key used by the Rust capacity gate (`date_naive` of committed_start). */
export function workDateFromCommittedIso(iso: string): string | null {
  const ms = parseMs(iso);
  if (!ms) return null;
  return new Date(ms).toISOString().slice(0, 10);
}

export type TeamDayCapacity = {
  availableHours: number;
  overtimeHours: number;
  committedHours: number;
  remainingHours: number;
};

/**
 * Remaining team capacity for a work date (mirrors compute_capacity_gate defaults).
 * Missing gantt rows ⇒ 8h available / 0 overtime / 0 committed.
 */
export function resolveTeamDayCapacity(
  capacity: TeamCapacityLoad[] | null | undefined,
  teamId: number,
  workDate: string,
): TeamDayCapacity {
  const row = (capacity ?? []).find((c) => c.team_id === teamId && c.work_date === workDate);
  const availableHours = row?.available_hours ?? PLANNING_DEFAULT_DAILY_CAPACITY_HOURS;
  const overtimeHours = row?.overtime_hours ?? 0;
  const committedHours = row?.committed_hours ?? 0;
  const remainingHours = Math.max(0, availableHours + overtimeHours - committedHours);
  return { availableHours, overtimeHours, committedHours, remainingHours };
}

/** True when requested hours would fail the daily capacity gate. */
export function exceedsTeamDayCapacity(
  capacity: TeamCapacityLoad[] | null | undefined,
  teamId: number,
  committedStartIso: string,
  committedEndIso: string,
): boolean {
  const workDate = workDateFromCommittedIso(committedStartIso);
  if (!workDate) return true;
  const startMs = parseMs(committedStartIso);
  const endMs = parseMs(committedEndIso);
  if (!startMs || !endMs || endMs <= startMs) return true;
  const requested = (endMs - startMs) / 3_600_000;
  const { remainingHours } = resolveTeamDayCapacity(capacity, teamId, workDate);
  return requested > remainingHours + 1e-6;
}

/**
 * Clamp end so the slot fits remaining daily capacity (and optional hard max end ms).
 */
export function clampEndToRemainingCapacity(args: {
  startIso: string;
  endIso: string;
  teamId: number;
  capacity: TeamCapacityLoad[] | null | undefined;
  hardMaxEndMs?: number;
}): string {
  const { startIso, endIso, teamId, capacity, hardMaxEndMs } = args;
  const startMs = parseMs(startIso);
  const endMs = parseMs(endIso);
  if (!startMs || !endMs || endMs <= startMs) return endIso;

  const workDate = workDateFromCommittedIso(startIso);
  if (!workDate) return endIso;
  const { remainingHours } = resolveTeamDayCapacity(capacity, teamId, workDate);
  if (remainingHours <= 0) {
    // Keep a 1-minute marker so the field stays valid; submit will block.
    return toDateTimeLocal(new Date(startMs + 60_000).toISOString());
  }

  let maxEndMs = startMs + remainingHours * 3_600_000;
  if (hardMaxEndMs != null && hardMaxEndMs > startMs) {
    maxEndMs = Math.min(maxEndMs, hardMaxEndMs);
  }
  const nextEndMs = Math.min(endMs, maxEndMs);
  return toDateTimeLocal(new Date(nextEndMs).toISOString());
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

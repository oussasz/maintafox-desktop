import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import {
  buildPlanningTimelineItems,
  buildPlanningTimelineLanes,
  buildPlanningWindowHighlights,
  capacityByDate,
  commitmentItemId,
  teamGroupId,
  parseCommitmentItemId,
  parseTeamGroupId,
  PLANNING_UNASSIGNED_GROUP_ID,
  utilizationState,
} from "@/components/planning/planning-model";
import {
  ResourceTimeline,
  type ResourceTimelineGroup,
  type ResourceTimelineItem,
  type ResourceTimelineTimeRangeHighlight,
} from "@/components/timeline/ResourceTimeline";
import { Badge } from "@/components/ui/badge";
import { RESOURCE_TIMELINE_STATE_CLASS } from "@/design-system/resource-timeline-tokens";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import type {
  PlanningGanttSnapshot,
  PlanningWindow,
  ScheduleCommitment,
  TeamCapacityLoad,
} from "@shared/ipc-types";

export type PlanningBoardLaneMode = "crew" | "technician";

type PlanningBoardProps = {
  gantt: PlanningGanttSnapshot | null;
  commitments: ScheduleCommitment[];
  capacityLoad: TeamCapacityLoad[];
  lockedWindows: PlanningWindow[];
  visibleStart: number;
  visibleEnd: number;
  horizonStart: number;
  horizonEnd: number;
  onVisibleTimeChange: (start: number, end: number) => void;
  selectedCommitmentId: number | null;
  onSelectCommitment: (commitmentId: number) => void;
  onReschedule: (
    commitmentId: number,
    newStartMs: number,
    personnelId: number | null,
    teamId: number | null,
  ) => void;
  loading?: boolean;
  error?: string | null;
  canDrag?: boolean;
  /** crew = plant-wide All Crews; technician = single-crew drill-in */
  laneMode?: PlanningBoardLaneMode;
  /** Forces ResourceTimeline remount when DU/AU (or scope) changes. */
  remountKey?: string;
  maxZoom?: number;
};

export function PlanningBoard({
  gantt,
  commitments,
  capacityLoad,
  lockedWindows,
  visibleStart,
  visibleEnd,
  horizonStart,
  horizonEnd,
  onVisibleTimeChange,
  selectedCommitmentId,
  onSelectCommitment,
  onReschedule,
  loading = false,
  error = null,
  canDrag = true,
  laneMode = "technician",
  remountKey,
  maxZoom,
}: PlanningBoardProps) {
  const { t } = useTranslation("planning");

  const groups = useMemo<ResourceTimelineGroup[]>(() => {
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
              {lane.team_code ? (
                <span className="truncate text-[10px] text-text-muted">{lane.team_code}</span>
              ) : null}
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

  const items = useMemo<ResourceTimelineItem[]>(() => {
    if (laneMode === "crew") {
      const next: ResourceTimelineItem[] = [];
      for (const commitment of commitments) {
        const start = Date.parse(commitment.committed_start);
        const end = Date.parse(commitment.committed_end);
        if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) continue;
        const frozen = Boolean(commitment.frozen_at);
        const title =
          commitment.source_code?.trim() ||
          commitment.source_title?.trim() ||
          commitment.source_type;
        next.push({
          id: commitmentItemId(commitment.id),
          group: teamGroupId(commitment.assigned_team_id),
          title,
          start_time: start,
          end_time: end,
          state: frozen ? "blocked" : commitment.has_blocking_conflict ? "critical" : "commitment",
          canMove: canDrag && !frozen,
        });
      }
      return next;
    }

    return buildPlanningTimelineItems({ commitments, gantt, canDrag }).map((row) => ({
      id: row.id,
      group: row.groupId,
      title: row.title,
      start_time: row.startMs,
      end_time: row.endMs,
      state: row.state,
      canMove: row.canMove,
    }));
  }, [commitments, gantt, canDrag, laneMode]);

  const highlights = useMemo<ResourceTimelineTimeRangeHighlight[]>(
    () => buildPlanningWindowHighlights(lockedWindows),
    [lockedWindows],
  );

  const capacityMap = useMemo(() => capacityByDate(capacityLoad), [capacityLoad]);
  const capacityDays = useMemo(() => {
    const days: string[] = [];
    const cursor = new Date(horizonStart);
    const end = new Date(horizonEnd);
    cursor.setHours(0, 0, 0, 0);
    end.setHours(0, 0, 0, 0);
    while (cursor <= end) {
      const y = cursor.getFullYear();
      const m = String(cursor.getMonth() + 1).padStart(2, "0");
      const d = String(cursor.getDate()).padStart(2, "0");
      days.push(`${y}-${m}-${d}`);
      cursor.setDate(cursor.getDate() + 1);
    }
    return days;
  }, [horizonStart, horizonEnd]);

  /** Capacity rows of the strip: one per crew in All-crews mode, the selected crew otherwise. */
  const capacityRows = useMemo(() => {
    if (laneMode !== "crew")
      return [{ teamId: null as number | null, name: "", byDay: capacityMap }];
    const byTeam = new Map<number, { name: string; byDay: Map<string, TeamCapacityLoad> }>();
    for (const lane of gantt?.crew_lanes ?? []) {
      byTeam.set(lane.team_id, { name: lane.team_name, byDay: new Map() });
    }
    for (const row of capacityLoad) {
      const entry = byTeam.get(row.team_id) ?? { name: t("board.unknownCrew"), byDay: new Map() };
      entry.byDay.set(row.work_date.slice(0, 10), row);
      byTeam.set(row.team_id, entry);
    }
    return [...byTeam.entries()].map(([teamId, v]) => ({ teamId: teamId as number | null, ...v }));
  }, [laneMode, gantt?.crew_lanes, capacityLoad, capacityMap, t]);

  const renderCapacityDay = (day: string, row: TeamCapacityLoad | undefined) => {
    if (!row) {
      return (
        <div
          key={day}
          className="shrink-0 rounded border border-surface-border px-2 py-1 text-[10px] text-text-muted"
        >
          {day.slice(5)}
        </div>
      );
    }
    const basisLabel =
      row.basis && row.basis !== "roster"
        ? t(`commitment.capacityBasis.${row.basis}`, { defaultValue: row.basis })
        : null;
    // No hours that day: say why; committed work on such a day is an overload.
    const state =
      row.utilization_ratio == null
        ? row.committed_hours > 0
          ? "critical"
          : "free"
        : utilizationState(row.utilization_ratio);
    return (
      <div
        key={day}
        className={cn(
          "shrink-0 rounded border px-2 py-1 text-[10px]",
          RESOURCE_TIMELINE_STATE_CLASS[state],
        )}
        title={
          t("gantt.capacityLine", {
            committed: row.committed_hours.toFixed(1),
            available: (row.available_hours + row.overtime_hours).toFixed(1),
          }) + (basisLabel ? ` · ${basisLabel}` : "")
        }
      >
        <span className="font-medium">{day.slice(5)}</span>{" "}
        <Badge variant="outline" className="h-4 px-1 text-[9px]">
          {row.utilization_ratio == null
            ? t("gantt.noCapacity")
            : `${Math.round(row.utilization_ratio * 100)}%`}
        </Badge>
      </div>
    );
  };

  const emptyTitle =
    commitments.length === 0
      ? laneMode === "crew"
        ? t("board.emptyAllCrews")
        : t("board.emptyWithCrew")
      : t("board.empty");

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="space-y-1.5 px-1">
        <p className="text-[10px] font-medium uppercase tracking-wide text-text-muted">
          {laneMode === "crew" ? t("gantt.capacityTitleAllCrews") : t("gantt.capacityTitle")}
        </p>
        {capacityLoad.length === 0 ? (
          <span className="text-xs text-text-muted">{t("gantt.emptyCapacity")}</span>
        ) : (
          <div className="space-y-1 overflow-x-auto pb-1">
            {capacityRows.map((row) => (
              <div key={row.teamId ?? "selected"} className="flex items-center gap-1.5">
                {laneMode === "crew" ? (
                  <span className="w-28 shrink-0 truncate text-[10px] font-medium text-text-secondary">
                    {row.name}
                  </span>
                ) : null}
                {capacityDays.map((day) => renderCapacityDay(day, row.byDay.get(day)))}
              </div>
            ))}
          </div>
        )}
      </div>

      {lockedWindows.filter((w) => w.is_locked === 1).length > 0 && (
        <div className="px-1 text-[11px] text-text-secondary">
          {t("gantt.freezeOverlayTitle")}:{" "}
          {lockedWindows
            .filter((w) => w.is_locked === 1)
            .map((w) => formatOrDash(w.lock_reason ?? w.window_type))
            .join(" · ")}
        </div>
      )}

      <ResourceTimeline
        key={remountKey}
        groups={groups}
        items={items}
        visibleTimeStart={visibleStart}
        visibleTimeEnd={visibleEnd}
        horizonStart={horizonStart}
        horizonEnd={horizonEnd}
        onVisibleTimeChange={onVisibleTimeChange}
        selectedItemIds={
          selectedCommitmentId != null ? [commitmentItemId(selectedCommitmentId)] : []
        }
        onItemSelect={(itemId) => {
          const id = parseCommitmentItemId(itemId);
          if (id != null) onSelectCommitment(id);
        }}
        {...(canDrag
          ? {
              onItemMove: (
                itemId: string | number,
                newStartMs: number,
                newGroupId: string | number,
              ) => {
                const id = parseCommitmentItemId(itemId);
                if (id == null) return;
                if (laneMode === "crew") {
                  const teamId = parseTeamGroupId(newGroupId);
                  onReschedule(id, newStartMs, null, teamId);
                  return;
                }
                const personnelId =
                  String(newGroupId) === PLANNING_UNASSIGNED_GROUP_ID ? null : Number(newGroupId);
                onReschedule(
                  id,
                  newStartMs,
                  personnelId != null && Number.isFinite(personnelId) ? personnelId : null,
                  null,
                );
              },
            }
          : {})}
        timeRangeHighlights={highlights}
        loading={loading}
        error={error}
        emptyTitle={emptyTitle}
        className="min-h-[320px] flex-1"
        {...(maxZoom != null ? { maxZoom } : {})}
      />
    </div>
  );
}

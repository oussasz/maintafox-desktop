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

  const crewCapacitySummary = useMemo(() => {
    if (laneMode !== "crew") return [];
    const byTeam = new Map<number, { name: string; maxUtil: number }>();
    for (const lane of gantt?.crew_lanes ?? []) {
      byTeam.set(lane.team_id, { name: lane.team_name, maxUtil: 0 });
    }
    for (const row of capacityLoad) {
      const entry = byTeam.get(row.team_id);
      if (!entry) {
        byTeam.set(row.team_id, {
          name: t("board.unknownCrew"),
          maxUtil: row.utilization_ratio,
        });
        continue;
      }
      entry.maxUtil = Math.max(entry.maxUtil, row.utilization_ratio);
    }
    return [...byTeam.entries()].map(([teamId, v]) => ({
      teamId,
      name: v.name,
      utilization: v.maxUtil,
    }));
  }, [laneMode, gantt?.crew_lanes, capacityLoad, t]);

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
        {laneMode === "crew" ? (
          <div className="flex flex-wrap gap-1.5">
            {crewCapacitySummary.length === 0 ? (
              <span className="text-xs text-text-muted">{t("gantt.emptyCapacity")}</span>
            ) : (
              crewCapacitySummary.map((row) => {
                const state = utilizationState(row.utilization);
                return (
                  <div
                    key={row.teamId}
                    className={cn(
                      "rounded border px-2 py-1 text-[10px]",
                      RESOURCE_TIMELINE_STATE_CLASS[state],
                    )}
                  >
                    <span className="font-medium">{row.name}</span>{" "}
                    <Badge variant="outline" className="h-4 px-1 text-[9px]">
                      {Math.round(row.utilization * 100)}%
                    </Badge>
                  </div>
                );
              })
            )}
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {capacityDays.slice(0, 28).map((day) => {
              const row = capacityMap.get(day);
              if (!row) {
                return (
                  <div
                    key={day}
                    className="rounded border border-surface-border px-2 py-1 text-[10px] text-text-muted"
                  >
                    {day.slice(5)}
                  </div>
                );
              }
              const state = utilizationState(row.utilization_ratio);
              return (
                <div
                  key={day}
                  className={cn(
                    "rounded border px-2 py-1 text-[10px]",
                    RESOURCE_TIMELINE_STATE_CLASS[state],
                  )}
                  title={t("gantt.capacityLine", {
                    committed: row.committed_hours.toFixed(1),
                    available: (row.available_hours + row.overtime_hours).toFixed(1),
                  })}
                >
                  <span className="font-medium">{day.slice(5)}</span>{" "}
                  <Badge variant="outline" className="h-4 px-1 text-[9px]">
                    {Math.round(row.utilization_ratio * 100)}%
                  </Badge>
                </div>
              );
            })}
            {capacityLoad.length === 0 && (
              <span className="text-xs text-text-muted">{t("gantt.emptyCapacity")}</span>
            )}
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

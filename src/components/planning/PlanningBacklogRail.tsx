import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { ConflictReasonList } from "@/components/planning/ConflictReasonList";
import {
  candidateDisplayCode,
  candidateDisplayTitle,
  candidateSourcePath,
  isBlockingSeverity,
} from "@/components/planning/planning-model";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { WoShutdownBadge } from "@/components/wo/WoShutdownRequirementField";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import type {
  CandidateConflictSummary,
  ConflictReason,
  ScheduleCandidate,
} from "@shared/ipc-types";

type PlanningBacklogRailProps = {
  ready: ScheduleCandidate[];
  blocked: ScheduleCandidate[];
  conflictSummary: CandidateConflictSummary[];
  selectedCandidateId: number | null;
  onSelectReady: (candidate: ScheduleCandidate) => void;
  /** Opens the planning windows from a reason that needs one. */
  onOpenWindows?: () => void;
  className?: string;
};

export function PlanningBacklogRail({
  ready,
  blocked,
  conflictSummary,
  selectedCandidateId,
  onSelectReady,
  onOpenWindows,
  className,
}: PlanningBacklogRailProps) {
  const { t } = useTranslation("planning");
  const navigate = useNavigate();
  const [tab, setTab] = useState<"ready" | "blocked">("ready");

  const conflictByCandidate = useMemo(() => {
    const map = new Map<number, CandidateConflictSummary>();
    for (const row of conflictSummary) {
      map.set(row.candidate_id, row);
    }
    return map;
  }, [conflictSummary]);

  return (
    <aside
      className={cn(
        "flex h-full min-h-0 w-full max-w-[360px] shrink-0 flex-col border-r border-surface-border bg-surface-1",
        className,
      )}
    >
      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v === "blocked" ? "blocked" : "ready")}
        className="flex min-h-0 flex-1 flex-col"
      >
        <div className="border-b border-surface-border px-3 pt-3 pb-2">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="ready" className="text-xs">
              {t("backlog.readyColumn")} ({ready.length})
            </TabsTrigger>
            <TabsTrigger value="blocked" className="text-xs">
              {t("backlog.blockedColumn")} ({blocked.length})
            </TabsTrigger>
          </TabsList>
          {tab === "ready" ? (
            <p className="mt-2 text-[11px] leading-snug text-text-muted">
              {t("backlog.readyHint")}
            </p>
          ) : null}
        </div>

        <TabsContent value="ready" className="mt-0 min-h-0 flex-1 overflow-y-auto p-3">
          <div className="space-y-2">
            {ready.map((candidate) => {
              const selected = selectedCandidateId === candidate.id;
              const summary = conflictByCandidate.get(candidate.id);
              return (
                <div
                  key={candidate.id}
                  className={cn(
                    "rounded-lg border",
                    selected ? "border-primary bg-surface-2" : "border-surface-border bg-surface-0",
                  )}
                >
                  <button
                    type="button"
                    className="w-full rounded-lg p-3 text-left transition-colors hover:bg-surface-2"
                    onClick={() => onSelectReady(candidate)}
                  >
                    <CandidateCardBody candidate={candidate} summary={summary} />
                  </button>
                  <ConflictReasonList
                    reasons={summary?.reasons ?? []}
                    onOpenWindows={onOpenWindows}
                    className="px-3 pb-3"
                  />
                </div>
              );
            })}
            {ready.length === 0 && (
              <p className="text-xs text-text-muted">{t("backlog.emptyReady")}</p>
            )}
          </div>
        </TabsContent>

        <TabsContent value="blocked" className="mt-0 min-h-0 flex-1 overflow-y-auto p-3">
          <div className="space-y-2">
            {blocked.length > 0 ? (
              <p className="text-[11px] leading-snug text-text-muted">{t("backlog.blockedHint")}</p>
            ) : null}
            {blocked.map((candidate) => {
              const summary = conflictByCandidate.get(candidate.id);
              const body = <CandidateCardBody candidate={candidate} summary={summary} />;
              const path = candidateSourcePath(candidate);
              return (
                <div
                  key={candidate.id}
                  className="rounded-lg border border-surface-border bg-surface-0"
                >
                  {path ? (
                    <button
                      type="button"
                      className="w-full rounded-lg p-3 text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      aria-label={t("backlog.openSource", {
                        code: candidateDisplayCode(candidate),
                      })}
                      onClick={() => void navigate(path)}
                    >
                      {body}
                    </button>
                  ) : (
                    <div className="p-3">{body}</div>
                  )}
                  <ConflictReasonList
                    reasons={summary?.reasons ?? []}
                    onOpenWindows={onOpenWindows}
                    className="px-3 pb-3"
                  />
                </div>
              );
            })}
            {blocked.length === 0 && (
              <p className="text-xs text-text-muted">{t("backlog.emptyBlocked")}</p>
            )}
          </div>
        </TabsContent>
      </Tabs>
    </aside>
  );
}

function CandidateCardBody({
  candidate,
  summary,
}: {
  candidate: ScheduleCandidate;
  summary: CandidateConflictSummary | undefined;
}) {
  const { t } = useTranslation("planning");
  const code = candidateDisplayCode(candidate);
  const title = candidateDisplayTitle(candidate);
  // One badge per kind of conflict: red when one of its reasons blocks, amber when all warn.
  const reasons: ConflictReason[] = summary?.reasons ?? [];
  const kinds = [...new Set(summary?.blocker_codes ?? [])];
  const kindBlocks = (kind: string) => {
    const ofKind = reasons.filter((r) => r.conflict_type === kind);
    return ofKind.length === 0 || ofKind.some((r) => isBlockingSeverity(r.severity));
  };
  // Duration × crew size, or what is missing (no duration → not estimated; no crew → said so).
  const hours = candidate.estimated_duration_hours;
  const crew = candidate.planned_crew_size;
  const estimateLabel =
    hours == null || hours <= 0
      ? t("backlog.notEstimated")
      : crew == null || crew < 1
        ? t("backlog.noCrew", { hours: hours.toFixed(1) })
        : t("backlog.estimate", { hours: hours.toFixed(1), crew });

  return (
    <div className="space-y-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-text-primary">{code}</div>
          {title ? <div className="truncate text-xs text-text-secondary">{title}</div> : null}
        </div>
        <Badge variant="outline" className="shrink-0 text-[10px]">
          {t(`filters.sourceTypes.${candidate.source_type}`, {
            defaultValue: candidate.source_type,
          })}
        </Badge>
      </div>

      <div className="flex flex-wrap gap-1.5">
        <Badge variant="secondary" className="text-[10px]">
          {t("backlog.readinessScore")}: {candidate.readiness_score.toFixed(1)}
        </Badge>
        {candidate.skill_match_score != null && (
          <Badge variant="outline" className="text-[10px]">
            {t("backlog.skillMatch")}: {Math.round(candidate.skill_match_score * 100)}%
          </Badge>
        )}
        <Badge variant="outline" className="text-[10px]">
          {estimateLabel}
        </Badge>
        {candidate.priority_label ? (
          <Badge variant="outline" className="text-[10px]">
            {candidate.priority_label}
          </Badge>
        ) : null}
        {candidate.shutdown_requirement ? <WoShutdownBadge /> : null}
      </div>

      {kinds.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {kinds.slice(0, 6).map((codeKey) =>
            kindBlocks(codeKey) ? (
              <Badge key={codeKey} variant="destructive" className="text-[10px] font-normal">
                {t(`blockers.${codeKey}`, { defaultValue: codeKey })}
              </Badge>
            ) : (
              <Badge
                key={codeKey}
                variant="outline"
                className={cn("text-[10px] font-normal", mfTone.warning.chip)}
              >
                {t(`blockers.${codeKey}`, { defaultValue: codeKey })}
              </Badge>
            ),
          )}
        </div>
      )}
    </div>
  );
}

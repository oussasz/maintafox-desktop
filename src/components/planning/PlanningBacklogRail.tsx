import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { candidateDisplayCode, candidateDisplayTitle } from "@/components/planning/planning-model";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import type { CandidateConflictSummary, ScheduleCandidate } from "@shared/ipc-types";

type PlanningBacklogRailProps = {
  ready: ScheduleCandidate[];
  blocked: ScheduleCandidate[];
  conflictSummary: CandidateConflictSummary[];
  selectedCandidateId: number | null;
  onSelectReady: (candidate: ScheduleCandidate) => void;
  className?: string;
};

function riskTone(risk: number | null | undefined): "default" | "secondary" | "destructive" {
  if (risk == null) return "secondary";
  if (risk >= 0.7) return "destructive";
  if (risk >= 0.4) return "secondary";
  return "default";
}

export function PlanningBacklogRail({
  ready,
  blocked,
  conflictSummary,
  selectedCandidateId,
  onSelectReady,
  className,
}: PlanningBacklogRailProps) {
  const { t } = useTranslation("planning");
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
              return (
                <button
                  key={candidate.id}
                  type="button"
                  className={cn(
                    "w-full rounded-lg border p-3 text-left transition-colors hover:bg-surface-2",
                    selected ? "border-primary bg-surface-2" : "border-surface-border bg-surface-0",
                  )}
                  onClick={() => onSelectReady(candidate)}
                >
                  <CandidateCardBody
                    candidate={candidate}
                    blockers={conflictByCandidate.get(candidate.id)?.blocker_codes ?? []}
                  />
                </button>
              );
            })}
            {ready.length === 0 && (
              <p className="text-xs text-text-muted">{t("backlog.emptyReady")}</p>
            )}
          </div>
        </TabsContent>

        <TabsContent value="blocked" className="mt-0 min-h-0 flex-1 overflow-y-auto p-3">
          <div className="space-y-2">
            {blocked.map((candidate) => (
              <div
                key={candidate.id}
                className="rounded-lg border border-surface-border bg-surface-0 p-3"
              >
                <CandidateCardBody
                  candidate={candidate}
                  blockers={conflictByCandidate.get(candidate.id)?.blocker_codes ?? []}
                />
              </div>
            ))}
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
  blockers,
}: {
  candidate: ScheduleCandidate;
  blockers: string[];
}) {
  const { t } = useTranslation("planning");
  const code = candidateDisplayCode(candidate);
  const title = candidateDisplayTitle(candidate);

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
        {candidate.estimated_assignment_risk != null && (
          <Badge variant={riskTone(candidate.estimated_assignment_risk)} className="text-[10px]">
            {t("backlog.risk")}: {Math.round(candidate.estimated_assignment_risk * 100)}%
          </Badge>
        )}
        {candidate.priority_label ? (
          <Badge variant="outline" className="text-[10px]">
            {candidate.priority_label}
          </Badge>
        ) : null}
      </div>

      {blockers.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {blockers.slice(0, 6).map((codeKey) => (
            <Badge key={codeKey} variant="destructive" className="text-[10px] font-normal">
              {t(`blockers.${codeKey}`, { defaultValue: codeKey })}
            </Badge>
          ))}
        </div>
      )}
    </div>
  );
}

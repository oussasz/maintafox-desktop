import { useEffect, useState } from "react";

import { listAssigneeCapacity } from "@/services/planning-service";
import type { AssigneeCapacity } from "@shared/ipc-types";

type AssigneeCapacityQuery = {
  enabled: boolean;
  teamId: number | null;
  startIso: string;
  endIso: string;
  /** A job to commit (its hours per person give each member's refusals). */
  candidateId?: number | null;
  /** A commitment being moved (excluded from the figures; its hours per person). */
  commitmentId?: number | null;
};

/**
 * Each crew member over a slot: shift and hours left per day, net of all their work, and what a
 * commit would refuse (`list_assignee_capacity`). Debounced while the slot is being typed; the
 * previous figures stay on screen until the next ones arrive.
 */
export function useAssigneeCapacity({
  enabled,
  teamId,
  startIso,
  endIso,
  candidateId = null,
  commitmentId = null,
}: AssigneeCapacityQuery): Map<number, AssigneeCapacity> {
  const [byPerson, setByPerson] = useState<Map<number, AssigneeCapacity>>(new Map());
  useEffect(() => {
    if (!enabled || teamId == null || !startIso || !endIso || endIso <= startIso) {
      setByPerson(new Map());
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      listAssigneeCapacity({
        team_id: teamId,
        start: startIso,
        end: endIso,
        candidate_id: candidateId,
        commitment_id: commitmentId,
      })
        .then((rows) => {
          if (!cancelled) setByPerson(new Map(rows.map((r) => [r.personnel_id, r])));
        })
        .catch(() => {
          // The picker still lists the crew; the commit gate checks the hours anyway.
          if (!cancelled) setByPerson(new Map());
        });
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [enabled, teamId, startIso, endIso, candidateId, commitmentId]);
  return byPerson;
}

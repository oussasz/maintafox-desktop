/**
 * Whether the signed-in user carries out a work order (L5-08, CMMS Advisor 2026-11-05): its
 * assignee, a member of its team, its planner, or an `ot.approve` holder at its site. Anyone
 * else sees its execution actions (start, pause, resume, hold, parts, tools, downtime, task
 * results, close-out details, "Terminé", close, cancel) greyed with the reason, naming who it is
 * assigned to; their own hours stay open to them, and the creator of a draft may cancel it.
 *
 * The backend computes the standing with the rule its commands apply (`get_wo_execution_standing`),
 * so the screen never duplicates the team logic. Read again when the work order changes (its
 * assignment) and when roles change (`rbac-changed`). While it loads, or when it cannot be read,
 * nothing is greyed on this basis: the backend stays the gate.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getWoExecutionStanding } from "@/services/wo-service";
import type { WoExecutionStanding, WorkOrder } from "@shared/ipc-types";

export interface WoExecutionGate {
  /** The standing is read (neither loading nor failed). */
  known: boolean;
  /**
   * Why the execution actions are greyed, in the user's language; null when the user carries out
   * the work order, or while the standing is not known.
   */
  blocker: string | null;
  /**
   * Why cancelling is greyed: as `blocker`, except that the creator of a draft (not yet released)
   * cancels it (CMMS Advisor 2026-11-06).
   */
  cancelBlocker: string | null;
}

interface Loaded {
  key: string;
  standing: WoExecutionStanding | null;
}

export function useWoExecutionStanding(
  wo: Pick<WorkOrder, "id" | "row_version"> | null | undefined,
): WoExecutionGate {
  const { t } = useTranslation("errors");
  const [loaded, setLoaded] = useState<Loaded | undefined>(undefined);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    // The listener may attach after unmount: detach it as soon as it arrives then.
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("rbac-changed", () => setVersion((v) => v + 1))
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      // Outside the desktop shell (no event bridge): read once, as on any other screen.
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const woId = wo?.id ?? null;
  const key = woId == null ? "" : `${woId}:${wo?.row_version ?? ""}:${version}`;

  useEffect(() => {
    if (woId == null) return;
    let cancelled = false;
    void getWoExecutionStanding(woId)
      .then((standing) => {
        if (!cancelled) setLoaded({ key, standing });
      })
      .catch(() => {
        if (!cancelled) setLoaded({ key, standing: null });
      });
    return () => {
      cancelled = true;
    };
  }, [woId, key]);

  // Only the answer for this very work order, version and role set counts.
  const current = loaded?.key === key ? loaded.standing : null;
  if (current == null) return { known: false, blocker: null, cancelBlocker: null };
  const refusal = current.actor == null ? current.refusal : null;
  if (refusal == null) return { known: true, blocker: null, cancelBlocker: null };
  const textKey = `gateError.${refusal.code}`;
  const blocker = String(
    t(textKey, { ...refusal.params, defaultValue: String(t("generic.forbidden")) }),
  );
  return { known: true, blocker, cancelBlocker: current.own_draft ? null : blocker };
}

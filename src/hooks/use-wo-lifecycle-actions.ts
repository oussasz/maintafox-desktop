/**
 * Shared WO lifecycle IPC actions for the detail dialog footer.
 * Panels keep form-local saves; footer owns status transitions.
 */

import { useCallback, useState } from "react";

import { useSession } from "@/hooks/use-session";
import { appErrorReason } from "@/lib/app-error";
import {
  approvePlanning,
  evaluateWoReadiness,
  markWoReady,
  returnToPlanning,
  resumeWo,
  startWo,
  submitWo,
} from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder, WoReadinessCheck, WoReadinessResult } from "@shared/ipc-types";

function blockingMessagesFromReport(result: WoReadinessResult): string[] {
  return result.checks
    .filter((c) => c.blocking && c.outcome === "fail")
    .map((c) => (c.message?.trim() ? c.message : c.code));
}

export function needsPlanningApproval(checks: WoReadinessCheck[]): boolean {
  return checks.some((c) => c.code === "approval_required" && c.blocking && c.outcome === "fail");
}

export type MarkReadyResult = { ok: true } | { ok: false; blocking: string[] };

export function useWoLifecycleActions(wo: WorkOrder | null) {
  const { info } = useSession();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [readinessBlocking, setReadinessBlocking] = useState<string[]>([]);
  const [readinessReport, setReadinessReport] = useState<WoReadinessResult | null>(null);

  const actorId = info?.user_id ?? null;

  const refreshReadiness = useCallback(async () => {
    if (!wo || (wo.status_code ?? "") !== "planning") {
      setReadinessReport(null);
      setReadinessBlocking([]);
      return null;
    }
    try {
      const result = await evaluateWoReadiness({ wo_id: wo.id });
      setReadinessReport(result);
      setReadinessBlocking(blockingMessagesFromReport(result));
      return result;
    } catch (e) {
      setReadinessReport(null);
      setReadinessBlocking([]);
      setError(appErrorReason(e));
      return null;
    }
  }, [wo]);

  const submit = useCallback(async () => {
    if (!wo || !actorId) return;
    setBusy(true);
    setError(null);
    try {
      await submitWo({
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: wo.row_version,
      });
      await refreshActiveWo();
    } catch (e) {
      setError(appErrorReason(e));
    } finally {
      setBusy(false);
    }
  }, [wo, actorId, refreshActiveWo]);

  const markReady = useCallback(async (): Promise<MarkReadyResult> => {
    if (!wo || !actorId) {
      return { ok: false, blocking: [] };
    }
    setBusy(true);
    setError(null);
    try {
      const result = await evaluateWoReadiness({ wo_id: wo.id });
      setReadinessReport(result);
      const blocking = blockingMessagesFromReport(result);
      setReadinessBlocking(blocking);
      if (!result.can_mark_ready) {
        return { ok: false, blocking };
      }
      await markWoReady({
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: wo.row_version,
      });
      setReadinessReport(null);
      setReadinessBlocking([]);
      await refreshActiveWo();
      return { ok: true };
    } catch (e) {
      const message = appErrorReason(e);
      setError(message);
      return { ok: false, blocking: [message] };
    } finally {
      setBusy(false);
    }
  }, [wo, actorId, refreshActiveWo]);

  const approve = useCallback(async () => {
    if (!wo || !actorId) return;
    setBusy(true);
    setError(null);
    try {
      await approvePlanning({
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: wo.row_version,
      });
      await refreshActiveWo();
      await refreshReadiness();
    } catch (e) {
      setError(appErrorReason(e));
    } finally {
      setBusy(false);
    }
  }, [wo, actorId, refreshActiveWo, refreshReadiness]);

  const returnToPlan = useCallback(
    async (reason?: string | null) => {
      if (!wo || !actorId) return;
      setBusy(true);
      setError(null);
      try {
        await returnToPlanning({
          wo_id: wo.id,
          actor_id: actorId,
          expected_row_version: wo.row_version,
          reason: reason?.trim() || null,
        });
        await refreshActiveWo();
      } catch (e) {
        setError(appErrorReason(e));
      } finally {
        setBusy(false);
      }
    },
    [wo, actorId, refreshActiveWo],
  );

  const start = useCallback(async () => {
    if (!wo || !actorId) return;
    setBusy(true);
    setError(null);
    try {
      await startWo({
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: wo.row_version,
      });
      await refreshActiveWo();
    } catch (e) {
      setError(appErrorReason(e));
    } finally {
      setBusy(false);
    }
  }, [wo, actorId, refreshActiveWo]);

  /** `remainingHours`: the estimated remaining time entered for the permit gate (null: full duration). */
  const resume = useCallback(
    async (remainingHours: number | null = null) => {
      if (!wo || !actorId) return;
      setBusy(true);
      setError(null);
      try {
        await resumeWo({
          wo_id: wo.id,
          actor_id: actorId,
          expected_row_version: wo.row_version,
          estimated_remaining_hours: remainingHours,
        });
        await refreshActiveWo();
      } catch (e) {
        setError(appErrorReason(e));
      } finally {
        setBusy(false);
      }
    },
    [wo, actorId, refreshActiveWo],
  );

  return {
    busy,
    error,
    setError,
    actorId,
    readinessReport,
    readinessBlocking,
    refreshReadiness,
    needsApproval: readinessReport ? needsPlanningApproval(readinessReport.checks) : false,
    submit,
    markReady,
    approve,
    returnToPlan,
    start,
    resume,
  };
}

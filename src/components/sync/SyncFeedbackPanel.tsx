import { Download, LifeBuoy, Play, RefreshCw, ShieldAlert } from "lucide-react";
import { useMemo, useState } from "react";

import { SupportBundleDialog } from "@/components/SupportBundleDialog";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { ResolveSyncConflictInput, SyncConflictRecord } from "@shared/ipc-types";

const CONFLICT_ACTIONS: ResolveSyncConflictInput["action"][] = [
  "accept_local",
  "accept_remote",
  "merge_fields",
  "retry_later",
  "escalate",
  "dismiss",
];

function downloadJson(filename: string, payload: unknown) {
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

function resolveLabel(conflict: SyncConflictRecord) {
  return `${conflict.entity_type}:${conflict.entity_sync_id} (${conflict.conflict_type})`;
}

export function SyncFeedbackPanel() {
  const state = useSyncOrchestratorStore((s) => s.runtimeState);
  const pendingBacklog = useSyncOrchestratorStore((s) => s.pendingBacklog);
  const unresolvedConflicts = useSyncOrchestratorStore((s) => s.unresolvedConflicts);
  const blockerReason = useSyncOrchestratorStore((s) => s.blockerReason);
  const lastSuccessAt = useSyncOrchestratorStore((s) => s.lastSuccessAt);
  const nextScheduledAt = useSyncOrchestratorStore((s) => s.nextScheduledAt);
  const policy = useSyncOrchestratorStore((s) => s.policy);
  const timeline = useSyncOrchestratorStore((s) => s.timeline);
  const conflictInbox = useSyncOrchestratorStore((s) => s.conflictInbox);
  const retry = useSyncOrchestratorStore((s) => s.retry);
  const runSyncNow = useSyncOrchestratorStore((s) => s.runSyncNow);
  const runReplayRecovery = useSyncOrchestratorStore((s) => s.runReplayRecovery);
  const resolveConflictAction = useSyncOrchestratorStore((s) => s.resolveConflictAction);
  const refreshInbox = useSyncOrchestratorStore((s) => s.refreshInbox);
  const pauseForMinutes = useSyncOrchestratorStore((s) => s.pauseForMinutes);
  const resumeNow = useSyncOrchestratorStore((s) => s.resumeNow);
  const exportDiagnostics = useSyncOrchestratorStore((s) => s.exportDiagnostics);
  const updatePolicy = useSyncOrchestratorStore((s) => s.updatePolicy);

  const [pauseMinutes, setPauseMinutes] = useState(15);
  const [supportOpen, setSupportOpen] = useState(false);
  const [resolvingById, setResolvingById] = useState<Record<number, ResolveSyncConflictInput["action"]>>({});

  const healthTone = useMemo(() => {
    if (state === "error") return "text-status-error";
    if (state === "degraded" || state === "blocked" || state === "paused") return "text-status-warning";
    if (state === "running") return "text-primary";
    return "text-status-success";
  }, [state]);

  const recentTimeline = useMemo(() => timeline.slice(-15).reverse(), [timeline]);

  const handleResolve = async (conflict: SyncConflictRecord) => {
    const suggested = CONFLICT_ACTIONS.includes(
      conflict.recommended_action as ResolveSyncConflictInput["action"],
    )
      ? (conflict.recommended_action as ResolveSyncConflictInput["action"])
      : "merge_fields";
    const action = resolvingById[conflict.id] ?? suggested;
    await resolveConflictAction({
      conflict_id: conflict.id,
      expected_row_version: conflict.row_version,
      action,
      resolution_note: `Resolved by operator using '${action}' action.`,
    });
  };

  return (
    <section className="rounded-xl border border-surface-border bg-surface-1 p-4 space-y-4">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-text-primary">Client Sync Orchestrator</h2>
          <p className="text-xs text-text-muted">
            Explicit runtime modes, health diagnostics, and operator remediation controls.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => void runSyncNow("manual")}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Retry now
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => void runReplayRecovery()}
          >
            <Play className="h-3.5 w-3.5" />
            Replay recovery
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => downloadJson(`sync-diagnostics-${Date.now()}.json`, exportDiagnostics())}
          >
            <Download className="h-3.5 w-3.5" />
            Export diagnostics
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => setSupportOpen(true)}
          >
            <LifeBuoy className="h-3.5 w-3.5" />
            Support handoff
          </button>
        </div>
      </header>

      <div className="grid gap-2 md:grid-cols-4">
        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <p className="text-2xs text-text-muted">Health state</p>
          <p className={`text-sm font-semibold capitalize ${healthTone}`}>{state}</p>
          {blockerReason && <p className="text-2xs text-status-warning mt-1">{blockerReason}</p>}
        </div>
        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <p className="text-2xs text-text-muted">Pending backlog</p>
          <p className="text-sm font-semibold text-text-primary">{pendingBacklog}</p>
        </div>
        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <p className="text-2xs text-text-muted">Unresolved conflicts</p>
          <p className="text-sm font-semibold text-text-primary">{unresolvedConflicts}</p>
        </div>
        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <p className="text-2xs text-text-muted">Last success</p>
          <p className="text-sm font-semibold text-text-primary">
            {lastSuccessAt ? new Date(lastSuccessAt).toLocaleString() : "Never"}
          </p>
          {nextScheduledAt && (
            <p className="text-2xs text-text-muted mt-1">
              Next run: {new Date(nextScheduledAt).toLocaleTimeString()}
            </p>
          )}
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        <div className="rounded-md border border-surface-border bg-surface-0 p-3 space-y-2">
          <h3 className="text-sm font-medium text-text-primary">Policy-aware scheduling</h3>
          <div className="flex flex-wrap items-center gap-2">
            <label className="text-2xs text-text-muted">
              Entitlement
              <select
                className="ml-2 rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs"
                value={policy.entitlementStatus}
                onChange={(e) =>
                  updatePolicy({ entitlementStatus: e.currentTarget.value as typeof policy.entitlementStatus })
                }
              >
                <option value="active">active</option>
                <option value="suspended">suspended</option>
                <option value="blocked">blocked</option>
              </select>
            </label>
            <label className="text-2xs text-text-muted">
              Bandwidth
              <select
                className="ml-2 rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs"
                value={policy.bandwidthMode}
                onChange={(e) => updatePolicy({ bandwidthMode: e.currentTarget.value as typeof policy.bandwidthMode })}
              >
                <option value="normal">normal</option>
                <option value="constrained">constrained</option>
              </select>
            </label>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="number"
              min={1}
              max={240}
              value={pauseMinutes}
              className="w-20 rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs"
              onChange={(e) => setPauseMinutes(Number(e.currentTarget.value))}
            />
            <button
              type="button"
              className="rounded border border-surface-border px-2 py-1 text-xs hover:bg-surface-2"
              onClick={() => pauseForMinutes(pauseMinutes)}
            >
              Pause sync
            </button>
            <button
              type="button"
              className="rounded border border-surface-border px-2 py-1 text-xs hover:bg-surface-2"
              onClick={resumeNow}
            >
              Resume sync
            </button>
            <button
              type="button"
              className="rounded border border-surface-border px-2 py-1 text-xs hover:bg-surface-2"
              onClick={() => void refreshInbox()}
            >
              Refresh inbox
            </button>
          </div>
          {retry.nextRetryAt && (
            <p className="text-2xs text-status-warning">
              Retry attempt {retry.attempt} scheduled at {new Date(retry.nextRetryAt).toLocaleTimeString()}.
            </p>
          )}
        </div>

        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <h3 className="text-sm font-medium text-text-primary mb-2">Timeline diagnostics</h3>
          <div className="max-h-56 overflow-auto space-y-2">
            {recentTimeline.length === 0 && (
              <p className="text-xs text-text-muted">No diagnostics events captured yet.</p>
            )}
            {recentTimeline.map((event) => (
              <div key={event.id} className="rounded border border-surface-border p-2">
                <div className="flex items-center justify-between gap-2">
                  <p className="text-xs font-medium text-text-primary">{event.event}</p>
                  <span className="text-2xs text-text-muted">{new Date(event.at).toLocaleTimeString()}</span>
                </div>
                <p className="text-2xs text-text-muted mt-0.5">{event.message}</p>
                <p className="text-[10px] text-text-muted mt-1">correlation: {event.correlationId}</p>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="rounded-md border border-surface-border bg-surface-0 p-3">
        <h3 className="text-sm font-medium text-text-primary mb-2 flex items-center gap-2">
          <ShieldAlert className="h-4 w-4 text-status-warning" />
          Conflict review inbox
        </h3>
        <div className="space-y-2 max-h-64 overflow-auto">
          {conflictInbox.length === 0 && (
            <p className="text-xs text-text-muted">No unresolved conflicts in the operator inbox.</p>
          )}
          {conflictInbox.map((conflict) => (
            <div key={conflict.id} className="rounded border border-surface-border p-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-xs font-medium text-text-primary">{resolveLabel(conflict)}</p>
                <span className="text-2xs text-text-muted capitalize">{conflict.status}</span>
              </div>
              <p className="text-2xs text-text-muted mt-1">
                recommended: <strong>{conflict.recommended_action}</strong>
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <select
                  className="rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs"
                  value={resolvingById[conflict.id] ?? conflict.recommended_action}
                  onChange={(e) =>
                    setResolvingById((current) => ({
                      ...current,
                      [conflict.id]: e.currentTarget.value as ResolveSyncConflictInput["action"],
                    }))
                  }
                >
                  {CONFLICT_ACTIONS.map((action) => (
                    <option key={action} value={action}>
                      {action}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="rounded border border-surface-border px-2 py-1 text-xs hover:bg-surface-2"
                  onClick={() => void handleResolve(conflict)}
                >
                  Apply resolution
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>

      <SupportBundleDialog open={supportOpen} onClose={() => setSupportOpen(false)} />
    </section>
  );
}

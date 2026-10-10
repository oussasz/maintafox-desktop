import { Download, LifeBuoy, Play, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { SupportBundleDialog } from "@/components/SupportBundleDialog";
import { SyncWorkerStatusCard } from "@/components/sync/SyncWorkerStatusCard";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { ResolveSyncConflictInput, SyncConflictRecord } from "@shared/ipc-types";

const CONFLICT_ACTIONS: ResolveSyncConflictInput["action"][] = [
  "accept_local",
  "accept_remote",
  "retry_later",
  "escalate",
  "dismiss",
];

const KNOWN_ACTION_LABELS = new Set<string>([...CONFLICT_ACTIONS, "review"]);

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

/** Settings → sync diagnostics: worker status, recovery actions and the conflict list. */
export function SyncFeedbackPanel() {
  const { t } = useTranslation("shell");
  const conflictInbox = useSyncOrchestratorStore((s) => s.conflictInbox);
  const runReplayRecovery = useSyncOrchestratorStore((s) => s.runReplayRecovery);
  const resolveConflictAction = useSyncOrchestratorStore((s) => s.resolveConflictAction);
  const refreshInbox = useSyncOrchestratorStore((s) => s.refreshInbox);
  const exportDiagnostics = useSyncOrchestratorStore((s) => s.exportDiagnostics);

  const [supportOpen, setSupportOpen] = useState(false);
  const [resolvingById, setResolvingById] = useState<
    Record<number, ResolveSyncConflictInput["action"]>
  >({});

  useEffect(() => {
    void refreshInbox().catch(() => {});
  }, [refreshInbox]);

  const actionLabel = (action: string) =>
    t(`sync.panel.action.${KNOWN_ACTION_LABELS.has(action) ? action : "review"}`);

  const handleResolve = async (conflict: SyncConflictRecord) => {
    const suggested = CONFLICT_ACTIONS.includes(
      conflict.recommended_action as ResolveSyncConflictInput["action"],
    )
      ? (conflict.recommended_action as ResolveSyncConflictInput["action"])
      : "escalate";
    const action = resolvingById[conflict.id] ?? suggested;
    await resolveConflictAction({
      conflict_id: conflict.id,
      expected_row_version: conflict.row_version,
      action,
      resolution_note: t("sync.panel.resolutionNote", { action: actionLabel(action) }),
    });
  };

  return (
    <section className="rounded-xl border border-surface-border bg-surface-1 p-4 space-y-4">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-text-primary">{t("sync.panel.title")}</h2>
          <p className="text-xs text-text-muted">{t("sync.panel.description")}</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => void runReplayRecovery()}
          >
            <Play className="h-3.5 w-3.5" />
            {t("sync.panel.replay")}
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => downloadJson(`sync-diagnostics-${Date.now()}.json`, exportDiagnostics())}
          >
            <Download className="h-3.5 w-3.5" />
            {t("sync.panel.export")}
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-2 rounded-md border border-surface-border px-3 py-1.5 text-xs hover:bg-surface-2"
            onClick={() => setSupportOpen(true)}
          >
            <LifeBuoy className="h-3.5 w-3.5" />
            {t("sync.panel.support")}
          </button>
        </div>
      </header>

      <SyncWorkerStatusCard />

      <div className="rounded-md border border-surface-border bg-surface-0 p-3">
        <h3 className="text-sm font-medium text-text-primary mb-2 flex items-center gap-2">
          <ShieldAlert className="h-4 w-4 text-status-warning" />
          {t("sync.panel.conflictsTitle")}
        </h3>
        <div className="space-y-2 max-h-64 overflow-auto">
          {conflictInbox.length === 0 && (
            <p className="text-xs text-text-muted">{t("sync.noConflicts")}</p>
          )}
          {conflictInbox.map((conflict) => (
            <div key={conflict.id} className="rounded border border-surface-border p-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-xs font-medium text-text-primary">{resolveLabel(conflict)}</p>
                <span className="text-2xs text-text-muted capitalize">{conflict.status}</span>
              </div>
              <p className="text-2xs text-text-muted mt-1">
                {t("sync.panel.recommended", { action: actionLabel(conflict.recommended_action) })}
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
                      {actionLabel(action)}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="rounded border border-surface-border px-2 py-1 text-xs hover:bg-surface-2"
                  onClick={() => void handleResolve(conflict)}
                >
                  {t("sync.panel.apply")}
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

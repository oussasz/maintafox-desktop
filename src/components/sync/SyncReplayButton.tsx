/**
 * "Send again" on one refused sync item (a conflict holding an outgoing change, or a change given
 * up after its delivery attempts). It names its item — there is no global replay of whatever comes
 * first — asks for confirmation, sends a translated reason with the request and says what was
 * done. Shown only to who holds `sync.replay` (the right `replay_sync_failures` checks).
 */
import { RotateCcw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { pushAppToast } from "@/store/app-toast-store";
import { type SyncReplayTarget, useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { ReplaySyncFailuresResult } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export interface SyncReplayButtonProps {
  target: SyncReplayTarget;
  /** The record named in the confirmation (its number, name or type). */
  record: string;
  disabled?: boolean;
}

/** What the replay actually did: a change queued again, only the conflict reopened, or nothing. */
function doneKey(
  result: ReplaySyncFailuresResult,
): "sync.replay.done" | "sync.replay.conflictOnly" | "sync.replay.nothing" {
  if (result.requeued_outbox_count > 0) return "sync.replay.done";
  if (result.transitioned_conflict_count > 0) return "sync.replay.conflictOnly";
  return "sync.replay.nothing";
}

export function SyncReplayButton({ target, record, disabled = false }: SyncReplayButtonProps) {
  const { t } = useTranslation("shell");
  const canReplay = usePermissions().can(P.SYNC_REPLAY);
  const replayItem = useSyncOrchestratorStore((s) => s.replayItem);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!canReplay) return null;

  const confirm = async () => {
    setError(null);
    try {
      const result = await replayItem(target, t("sync.replay.reason"));
      pushAppToast({ title: t(doneKey(result)), variant: "success" });
    } catch (err) {
      setError(appErrorReason(err));
      // Keeps the dialog open, with the reason, so the operator can try again.
      throw err;
    }
  };

  return (
    <>
      <Button
        type="button"
        variant="outline"
        className="h-10 gap-2"
        disabled={disabled}
        onClick={() => {
          setError(null);
          setOpen(true);
        }}
      >
        <RotateCcw className="h-4 w-4" aria-hidden />
        {t("sync.replay.action")}
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={t("sync.replay.confirmTitle")}
        description={t(
          target.kind === "conflict" ? "sync.replay.confirmConflict" : "sync.replay.confirmOutbox",
          { record },
        )}
        confirmLabel={t("sync.replay.action")}
        onConfirm={confirm}
      >
        {error ? (
          <p className="text-sm text-tone-danger-fg" role="alert">
            {t("sync.replay.failed", { message: error })}
          </p>
        ) : null}
      </ConfirmDialog>
    </>
  );
}

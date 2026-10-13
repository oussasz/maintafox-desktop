/**
 * Availability of synchronised files (attachments, photos, library documents) on this device.
 *
 * A file added on another device arrives with its record, then the sync worker downloads it.
 * Until then the lists show "not downloaded yet" instead of a broken file; a download that was
 * given up shows the reason and a retry action.
 */

import { AlertCircle, CloudDownload, FileQuestion, RotateCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui";
import { getMediaFileStates, retryMediaDownload } from "@/services/sync-service";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import { toErrorMessage } from "@/utils/errors";
import type { MediaFileState } from "@shared/ipc-types";

/**
 * File states of `relativePaths`, refreshed when the sync worker reports progress (a finished
 * download changes its pending count). Paths not answered yet are absent from the map.
 */
export function useMediaFileStates(relativePaths: string[]): {
  states: Map<string, MediaFileState>;
  refresh: () => Promise<void>;
} {
  const [states, setStates] = useState<Map<string, MediaFileState>>(new Map());
  const key = useMemo(() => [...relativePaths].sort().join("\n"), [relativePaths]);
  const downloadPending = useSyncOrchestratorStore(
    (s) => s.worker?.media_download_pending_count ?? 0,
  );
  const downloadFailed = useSyncOrchestratorStore(
    (s) => s.worker?.media_download_failed_count ?? 0,
  );
  const lastAttempt = useSyncOrchestratorStore((s) => s.worker?.last_attempt_at ?? null);

  const refresh = useCallback(async () => {
    const paths = key === "" ? [] : key.split("\n");
    if (paths.length === 0) {
      setStates(new Map());
      return;
    }
    try {
      const result = await getMediaFileStates(paths);
      setStates(new Map(result.map((s) => [s.relativePath, s])));
    } catch {
      // The lists still work without the state; the badge is simply not shown.
      setStates(new Map());
    }
  }, [key]);

  useEffect(() => {
    void refresh();
  }, [refresh, downloadPending, downloadFailed, lastAttempt]);

  return { states, refresh };
}

interface MediaFileStateBadgeProps {
  state: MediaFileState | undefined;
  /** Called after a retry was queued, to refresh the list. */
  onRetried?: () => void;
}

/** Nothing for an available (or not yet known) file; a short notice otherwise. */
export function MediaFileStateBadge({ state, onRetried }: MediaFileStateBadgeProps) {
  const { t } = useTranslation("shell");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const retry = useCallback(async () => {
    if (!state) return;
    setBusy(true);
    setMessage(null);
    try {
      const queued = await retryMediaDownload(state.relativePath);
      setMessage(t(queued > 0 ? "sync.mediaFile.retryQueued" : "sync.mediaFile.retryNothing"));
      onRetried?.();
    } catch (err) {
      setMessage(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [state, onRetried, t]);

  if (!state || state.state === "available") return null;

  if (state.state === "pending") {
    return (
      <span
        className="inline-flex items-center gap-1 text-xs text-text-muted"
        title={t("sync.mediaFile.pendingHint")}
        data-testid="media-file-pending"
      >
        <CloudDownload className="h-3.5 w-3.5" />
        {t("sync.mediaFile.pending")}
      </span>
    );
  }

  if (state.state === "missing") {
    return (
      <span
        className="inline-flex items-center gap-1 text-xs text-text-muted"
        title={t("sync.mediaFile.missingHint")}
        data-testid="media-file-missing"
      >
        <FileQuestion className="h-3.5 w-3.5" />
        {t("sync.mediaFile.missing")}
      </span>
    );
  }

  return (
    <span
      className="inline-flex flex-wrap items-center gap-1 text-xs text-status-danger"
      data-testid="media-file-failed"
    >
      <AlertCircle className="h-3.5 w-3.5" />
      <span title={t("sync.mediaFile.failedHint", { message: state.lastError ?? "" })}>
        {t("sync.mediaFile.failed")}
      </span>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 px-1.5 text-xs"
        disabled={busy}
        onClick={() => void retry()}
      >
        <RotateCw className="mr-1 h-3 w-3" />
        {t("sync.mediaFile.retry")}
      </Button>
      {message ? <span className="text-text-muted">{message}</span> : null}
    </span>
  );
}

/** Whether a file can be opened or previewed on this device. */
export function isMediaFileAvailable(state: MediaFileState | undefined): boolean {
  return state === undefined || state.state === "available";
}
